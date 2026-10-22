//! Join entry points bridging a parsed Datalog query to a [`Catalog`].
//!
//! A query runs in three steps, each a free function here: [`prepare`]
//! validates and plans it against the catalog's headers and statistics,
//! [`build`] builds the indexes the plan reads from the catalog's flat
//! tuples, and [`execute`] joins over them. Each iterator family has one
//! streaming function that runs all three — [`lftj_join_for_each`] for
//! sorted tries under a [`TrieIterable`]-family algorithm,
//! [`hash_join_for_each`] for [`HashTrieIterable`] structures under
//! [`HashTriejoin`] — plus a collecting wrapper ([`lftj_join`],
//! [`hash_join`]). Each is planned by a [`Planner`]. Both share one private
//! body and differ only in the bounds they put on the relation type and the
//! algorithm they run: an index is a plain relation, so no algorithm needs a
//! wrapper for a constant or a repeated variable.
//!
//! Every entry point is total: a query that cannot run over the catalog
//! returns a [`JoinError`] rather than panicking, identically for both
//! families. [`validate_query`] runs the same checks without joining, for
//! callers that want to reject a query before building or timing anything.
//! Runtime selection of the (structure, algorithm) cell lives in the CLI's
//! `execution` module, not here.

mod catalog;
mod lifecycle;
mod validation;

pub use {
    catalog::Catalog,
    lifecycle::{build, build_with, execute, prepare, Indexes, Prepared},
    validation::{validate_query, JoinError, RelationArities},
};
use {
    kermit_algos::{HashTriejoin, JoinAlgo, JoinQuery, Planner},
    kermit_ds::Relation,
    kermit_iters::{HashTrieIterable, TrieIterable},
};

/// The one join body: [`prepare`] (validation, accesses, statistics, plan,
/// index specs), [`build`] the indexes it names, [`execute`] over them.
///
/// Each result tuple is passed to `emit` as a borrowed slice of exactly the
/// head's columns; the result is never materialised here.
///
/// # Errors
///
/// Returns the [`JoinError`] [`validate_query`] would, or
/// [`JoinError::MissingStatistics`], before any index is built or any tuple
/// is emitted.
pub(crate) fn run_join<R, JA>(
    catalog: &Catalog, query: &JoinQuery, planner: &Planner, emit: impl FnMut(&[usize]),
) -> Result<(), JoinError>
where
    R: Relation,
    JA: JoinAlgo<R>,
{
    let prepared = prepare(catalog, query, planner)?;
    let indexes = build::<R>(catalog, &prepared);
    execute::<R, JA>(&prepared, &indexes, emit);
    Ok(())
}

/// Sorted-family join entry point: runs `query` over `catalog` with the
/// [`TrieIterable`]-family algorithm `JA` (normally
/// [`LeapfrogTriejoin`](kermit_algos::LeapfrogTriejoin)), planned by
/// `planner`, and passes each result tuple to `emit` without materialising
/// the result. `R` is the structure every index is built as. Mirror of
/// [`hash_join_for_each`].
///
/// # Errors
///
/// Returns a [`JoinError`], before emitting anything, if the query cannot
/// run over `catalog` (see [`validate_query`]) or if `planner` reads
/// statistics `catalog` was not built with
/// ([`JoinError::MissingStatistics`]).
pub fn lftj_join_for_each<R, JA>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner, emit: impl FnMut(&[usize]),
) -> Result<(), JoinError>
where
    R: Relation + TrieIterable,
    JA: JoinAlgo<R>,
{
    run_join::<R, JA>(catalog, &query, planner, emit)
}

/// [`lftj_join_for_each`], collected: returns every result tuple. For
/// callers that need the rows themselves; allocates one `Vec` per tuple.
///
/// # Errors
///
/// As [`lftj_join_for_each`].
pub fn lftj_join<R, JA>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner,
) -> Result<Vec<Vec<usize>>, JoinError>
where
    R: Relation + TrieIterable,
    JA: JoinAlgo<R>,
{
    let mut tuples = Vec::new();
    lftj_join_for_each::<R, JA>(catalog, query, planner, |tuple| tuples.push(tuple.to_vec()))?;
    Ok(tuples)
}

/// Hash-family join entry point: runs `query` over `catalog` with
/// [`HashTriejoin`], planned by `planner`, and passes each result tuple to
/// `emit` without materialising the result. `R` is the structure every index
/// is built as. Mirror of [`lftj_join_for_each`].
///
/// # Errors
///
/// As [`lftj_join_for_each`].
pub fn hash_join_for_each<R>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner, emit: impl FnMut(&[usize]),
) -> Result<(), JoinError>
where
    R: Relation + HashTrieIterable,
{
    run_join::<R, HashTriejoin>(catalog, &query, planner, emit)
}

/// [`hash_join_for_each`], collected: returns every result tuple. For
/// callers that need the rows themselves; allocates one `Vec` per tuple.
///
/// # Errors
///
/// As [`hash_join_for_each`].
pub fn hash_join<R>(
    catalog: &Catalog, query: JoinQuery, planner: &Planner,
) -> Result<Vec<Vec<usize>>, JoinError>
where
    R: Relation + HashTrieIterable,
{
    let mut tuples = Vec::new();
    hash_join_for_each::<R>(catalog, query, planner, |tuple| tuples.push(tuple.to_vec()))?;
    Ok(tuples)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_algos::{
            CatalogStats, LeapfrogTriejoin, LexicographicOptimiser, QueryOptimiser, QueryPlan,
            StatisticsLevel, ACCESS_PREFIX,
        },
        kermit_ds::{HashTrie, RelationHeader, TreeTrie, Tuples},
        std::{cell::RefCell, rc::Rc},
    };

    type Rows = Vec<Vec<usize>>;

    /// The planner every test plans with.
    fn planner_of(optimiser: impl QueryOptimiser + 'static) -> Planner {
        Planner::new(Box::new(optimiser))
    }

    /// A catalog of positional relations `(name, arity, rows)`, gathered up to
    /// `level`.
    fn store(level: StatisticsLevel, entries: Vec<(&str, usize, Rows)>) -> Catalog {
        let relations = entries
            .into_iter()
            .map(|(name, arity, rows)| {
                let tuples = if rows.is_empty() {
                    Tuples::new(arity)
                } else {
                    Tuples::from(rows)
                };
                (RelationHeader::new_positional(name, arity), tuples)
            })
            .collect();
        Catalog::new(relations, level)
    }

    /// [`store`] at [`StatisticsLevel::TupleCounts`], all that
    /// `LexicographicOptimiser` reads.
    fn counts(entries: Vec<(&str, usize, Rows)>) -> Catalog {
        store(StatisticsLevel::TupleCounts, entries)
    }

    fn sorted_family(catalog: &Catalog, query: &JoinQuery) -> Result<Rows, JoinError> {
        let planner = planner_of(LexicographicOptimiser);
        lftj_join::<TreeTrie, LeapfrogTriejoin>(catalog, query.clone(), &planner)
    }

    fn hash_family(catalog: &Catalog, query: &JoinQuery) -> Result<Rows, JoinError> {
        let planner = planner_of(LexicographicOptimiser);
        hash_join::<HashTrie>(catalog, query.clone(), &planner)
    }

    /// The rows `query` returns from each family, sorted. The families must
    /// agree, since no relation here holds a duplicate row.
    fn both_families(catalog: &Catalog, query: &str) -> Rows {
        let query: JoinQuery = query.parse().unwrap();
        let mut sorted = sorted_family(catalog, &query).unwrap();
        let mut hashed = hash_family(catalog, &query).unwrap();
        sorted.sort();
        hashed.sort();
        assert_eq!(sorted, hashed, "{query}");
        sorted
    }

    /// Plans like `lexicographic` but declares that it reads per-column
    /// distinct counts.
    struct NeedsColumns;

    impl QueryOptimiser for NeedsColumns {
        fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
            LexicographicOptimiser.plan(query, stats)
        }

        fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::ColumnDistinct }
    }

    /// Records the query and statistics `prepare` hands the optimiser, and
    /// plans like `lexicographic`. Clones share the record, so a test keeps
    /// one while the planner owns the other.
    #[derive(Default, Clone)]
    struct StatsSpy(Rc<RefCell<Option<(JoinQuery, CatalogStats)>>>);

    impl QueryOptimiser for StatsSpy {
        fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
            *self.0.borrow_mut() = Some((query.clone(), stats.clone()));
            LexicographicOptimiser.plan(query, stats)
        }

        fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::ColumnDistinct }
    }

    /// A query that fails validation reports that error, not
    /// `MissingStatistics`: the statistics check runs after validation.
    #[test]
    fn validation_errors_win_over_missing_statistics() {
        let catalog = counts(vec![("edge", 2, vec![vec![1, 2]])]);
        let query: JoinQuery = "Q(X) :- missing(X).".parse().unwrap();
        assert_eq!(
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&catalog, query, &planner_of(NeedsColumns)),
            Err(JoinError::UnknownRelation {
                relation: "missing".into(),
                known: vec!["edge".into()],
            })
        );
    }

    /// The planner is handed one statistics entry per access, keyed by the
    /// access id: counts over the rows the access's filter keeps for a
    /// filtered access, and the catalog's own for an unfiltered one. Answers
    /// cannot show this, since a plan never changes them, so it is checked on
    /// the statistics themselves, in both families.
    #[test]
    fn the_planner_reads_the_catalogs_statistics_per_access() {
        let catalog = store(StatisticsLevel::ColumnDistinct, vec![
            ("r", 2, vec![vec![1, 1], vec![1, 2], vec![2, 3], vec![3, 3]]),
            ("s", 2, vec![vec![1, 5], vec![3, 5], vec![3, 6]]),
            ("t", 1, vec![vec![1], vec![3], vec![4]]),
        ]);
        let query: JoinQuery = "Q(X) :- r(X, X), s(X, c5), t(X).".parse().unwrap();
        let check = |spy: &StatsSpy, mut got: Rows| {
            got.sort();
            assert_eq!(got, vec![vec![1], vec![3]]);
            let (planned, stats) = spy.0.borrow_mut().take().expect("the optimiser planned");
            let access = |k: usize| format!("{ACCESS_PREFIX}{k}");
            let names: Vec<String> = planned.body.iter().map(|atom| atom.name.clone()).collect();
            assert_eq!(names, (0..3).map(access).collect::<Vec<_>>());
            // r(X, X) keeps the diagonal rows (1, 1) and (3, 3) of four.
            assert_eq!(stats.tuples(&access(0)), Some(2));
            assert_eq!(stats.distinct(&access(0), 0), Some(2));
            // s(X, c5) keeps (1, 5) and (3, 5) of three.
            assert_eq!(stats.tuples(&access(1)), Some(2));
            assert_eq!(stats.distinct(&access(1), 0), Some(2));
            // t(X) is unfiltered: the catalog's own counts.
            assert_eq!(stats.tuples(&access(2)), Some(3));
            assert_eq!(stats.distinct(&access(2), 0), Some(3));
            // Statistics are keyed by access, never by the relation.
            assert_eq!(stats.tuples("r"), None);
        };

        let spy = StatsSpy::default();
        let rows = lftj_join::<TreeTrie, LeapfrogTriejoin>(
            &catalog,
            query.clone(),
            &planner_of(spy.clone()),
        )
        .unwrap();
        check(&spy, rows);

        let spy = StatsSpy::default();
        let rows = hash_join::<HashTrie>(&catalog, query, &planner_of(spy.clone())).unwrap();
        check(&spy, rows);
    }

    /// An optimiser that reads more than the catalog gathered is refused
    /// before any row, and the same relations gathered to its level run.
    #[test]
    fn an_optimiser_reading_ungathered_statistics_is_refused() {
        let entries = || {
            vec![
                ("first", 1, vec![vec![1], vec![2], vec![3]]),
                ("second", 1, vec![vec![2], vec![3], vec![4]]),
            ]
        };
        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        let mut rows = 0;
        let refused = lftj_join_for_each::<TreeTrie, LeapfrogTriejoin>(
            &counts(entries()),
            query.clone(),
            &planner_of(NeedsColumns),
            |_| rows += 1,
        );
        assert_eq!(
            refused,
            Err(JoinError::MissingStatistics {
                required: StatisticsLevel::ColumnDistinct,
                available: StatisticsLevel::TupleCounts,
            })
        );
        assert_eq!(rows, 0);

        let analysed = store(StatisticsLevel::ColumnDistinct, entries());
        let mut got =
            lftj_join::<TreeTrie, LeapfrogTriejoin>(&analysed, query, &planner_of(NeedsColumns))
                .unwrap();
        got.sort();
        assert_eq!(got, vec![vec![2], vec![3]]);
    }

    #[test]
    fn two_unary_relations_intersect() {
        let catalog = counts(vec![
            ("first", 1, vec![vec![1], vec![2], vec![3]]),
            ("second", 1, vec![vec![2], vec![3], vec![4]]),
        ]);
        let got = both_families(&catalog, "Q(X) :- first(X), second(X).");
        assert_eq!(got, vec![vec![2], vec![3]]);
    }

    /// A constant filters its column, in the first column as in the last.
    #[test]
    fn a_constant_filters_its_column() {
        let catalog = counts(vec![("p", 2, vec![vec![1, 10], vec![2, 20], vec![3, 30]])]);
        let got = both_families(&catalog, "Q(X) :- p(X, c10).");
        assert_eq!(
            got,
            vec![vec![1]],
            "expected only X=1 to pass the c10 filter, got {got:?}"
        );
        let got = both_families(&catalog, "Q(Y) :- p(c2, Y).");
        assert_eq!(got, vec![vec![20]]);
    }

    /// `Q(X) :- r(X, X).`: a repeated variable is an equality between its
    /// columns. Before the selection rewrite this returned every first-column
    /// key of `r`.
    #[test]
    fn a_repeated_variable_selects_the_diagonal() {
        let r = vec![vec![1, 1], vec![1, 2], vec![2, 3], vec![3, 3], vec![4, 5]];
        let catalog = counts(vec![("r", 2, r)]);
        let got = both_families(&catalog, "Q(X) :- r(X, X).");
        assert_eq!(got, vec![vec![1], vec![3]]);
    }

    /// One atom with a constant and a repeat: column 1 is 5 and column 0
    /// equals column 2.
    #[test]
    fn a_constant_and_a_repeat_share_an_atom() {
        let r = vec![vec![1, 5, 1], vec![1, 5, 2], vec![2, 6, 2], vec![3, 5, 3]];
        let catalog = counts(vec![("r", 3, r)]);
        let got = both_families(&catalog, "Q(X) :- r(X, c5, X).");
        assert_eq!(got, vec![vec![1], vec![3]]);
    }

    /// Two readings of one relation: `r(X, X)` is a filtered index while
    /// `r(X, Y)` reads it whole.
    #[test]
    fn one_relation_read_two_ways_in_one_query() {
        let r = vec![vec![1, 1], vec![1, 2], vec![2, 3], vec![3, 3]];
        let catalog = counts(vec![("r", 2, r)]);
        let got = both_families(&catalog, "Q(X, Y) :- r(X, X), r(X, Y).");
        assert_eq!(got, vec![vec![1, 1], vec![1, 2], vec![3, 3]]);
    }

    /// `Q(X) :- r(X, _).`: a trailing placeholder is a fresh unused variable,
    /// so `X` repeats once per matching tuple (bag semantics).
    #[test]
    fn a_trailing_placeholder_is_an_unused_variable() {
        let r = vec![vec![1, 2], vec![1, 3], vec![2, 4]];
        let catalog = counts(vec![("r", 2, r)]);
        let got = both_families(&catalog, "Q(X) :- r(X, _).");
        assert_eq!(got, vec![vec![1], vec![1], vec![2]]);
    }

    /// `Q(X, Y) :- r(X, _, Y).`: `Y` names the third column. Before the
    /// placeholder rewrite it bound the second (issue #73).
    #[test]
    fn a_middle_placeholder_keeps_later_columns_in_place() {
        let catalog = counts(vec![("r", 3, vec![vec![1, 2, 3], vec![1, 4, 5]])]);
        let got = both_families(&catalog, "Q(X, Y) :- r(X, _, Y).");
        assert_eq!(got, vec![vec![1, 3], vec![1, 5]]);
    }

    /// Rows carry exactly the head's columns, in head order: body-only
    /// variables, written or introduced by a placeholder, are dropped, and
    /// duplicates are kept (bag semantics), so the row count is unchanged by
    /// projection (#71).
    #[test]
    fn results_are_projected_to_the_head() {
        let catalog = counts(vec![
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
            assert_eq!(both_families(&catalog, q), want, "{q}");
        }
    }

    /// The streaming entry points visit every row without collecting;
    /// `lftj_join` and `hash_join` are the same traversals, collected.
    #[test]
    fn the_streaming_entry_points_visit_every_row() {
        let catalog = counts(vec![
            ("first", 1, vec![vec![1], vec![2], vec![3]]),
            ("second", 1, vec![vec![2], vec![3], vec![4]]),
        ]);
        let query: JoinQuery = "Q(X) :- first(X), second(X).".parse().unwrap();
        let planner = planner_of(LexicographicOptimiser);

        let mut sorted = Vec::new();
        lftj_join_for_each::<TreeTrie, LeapfrogTriejoin>(
            &catalog,
            query.clone(),
            &planner,
            |row| sorted.push(row.to_vec()),
        )
        .unwrap();
        sorted.sort();
        assert_eq!(sorted, vec![vec![2], vec![3]]);

        let mut hashed = Vec::new();
        hash_join_for_each::<HashTrie>(&catalog, query, &planner, |row| hashed.push(row.to_vec()))
            .unwrap();
        hashed.sort();
        assert_eq!(hashed, vec![vec![2], vec![3]]);
    }

    /// `missing` was never added. Silently dropping the atom would mask typos
    /// and load failures, and panicking exited 101; the entry points return
    /// the error instead, the same one from both families.
    #[test]
    fn an_unknown_relation_is_an_error_in_both_families() {
        let catalog = counts(vec![("edge", 2, vec![vec![1, 2]])]);
        let query: JoinQuery = "Q(X) :- missing(X).".parse().unwrap();
        let want: Result<Rows, JoinError> = Err(JoinError::UnknownRelation {
            relation: "missing".into(),
            known: vec!["edge".into()],
        });
        assert_eq!(sorted_family(&catalog, &query), want);
        assert_eq!(hash_family(&catalog, &query), want);
    }

    /// A rejected query emits nothing: validation runs before the join.
    #[test]
    fn a_rejected_query_emits_no_rows() {
        let catalog = counts(vec![("edge", 2, vec![vec![1, 2], vec![3, 4]])]);
        let query: JoinQuery = "Q(X, Y) :- edge(X, Z).".parse().unwrap();
        let planner = planner_of(LexicographicOptimiser);
        let mut rows = 0;
        let sorted = lftj_join_for_each::<TreeTrie, LeapfrogTriejoin>(
            &catalog,
            query.clone(),
            &planner,
            |_| rows += 1,
        );
        assert!(matches!(sorted, Err(JoinError::UnboundHeadVariable { .. })));
        let hashed = hash_join_for_each::<HashTrie>(&catalog, query, &planner, |_| rows += 1);
        assert!(matches!(hashed, Err(JoinError::UnboundHeadVariable { .. })));
        assert_eq!(rows, 0);
    }

    /// Every #78 shape is rejected by the hash family with exactly the error
    /// the sorted family gives: validation runs before either family's code.
    #[test]
    fn both_families_reject_malformed_queries_identically() {
        let edges = vec![vec![1, 2], vec![2, 1], vec![2, 3]];
        let catalog = counts(vec![("edge", 2, edges)]);
        for q in [
            "Q(X, Y) :- edge(X, Z).",
            "Q(X, X) :- edge(X, Y).",
            "Q(X, Y, Z) :- edge(X, Y, Z).",
            "Q(X) :- edge(X).",
            "Q(X, Y) :- nope(X, Y).",
            "Q(_) :- edge(X, Y).",
            "Q(X) :- edge(X, cfoo).",
            "Q(X) :- Const_c5(X).",
        ] {
            let query: JoinQuery = q.parse().unwrap();
            let from_hash = hash_family(&catalog, &query);
            let from_sorted = sorted_family(&catalog, &query);
            assert!(from_hash.is_err(), "{q} was accepted");
            assert_eq!(from_hash, from_sorted, "{q}");
        }
    }

    /// The mutual-edge graph: 1 <-> 2, 2 <-> 3 and the one-way 1 -> 3.
    fn mutual_edges() -> Rows { vec![vec![1, 2], vec![2, 1], vec![2, 3], vec![3, 2], vec![1, 3]] }

    /// `edge(X, Y), edge(Y, X)` reads `edge` in both column orders. The
    /// `stored` policy rejected it as a limitation; now each reading is an
    /// index in the plan's order, so one relation gives two indexes.
    #[test]
    fn a_self_join_in_opposite_orders_builds_two_indexes() {
        let catalog = counts(vec![("edge", 2, mutual_edges())]);
        let text = "Q(X, Y) :- edge(X, Y), edge(Y, X).";
        let got = both_families(&catalog, text);
        assert_eq!(got, vec![vec![1, 2], vec![2, 1], vec![2, 3], vec![3, 2]]);

        let query: JoinQuery = text.parse().unwrap();
        let prepared = prepare(&catalog, &query, &planner_of(LexicographicOptimiser)).unwrap();
        let mut columns: Vec<Vec<usize>> = prepared
            .indexes()
            .iter()
            .map(|spec| spec.columns.clone())
            .collect();
        columns.sort();
        assert_eq!(columns, vec![vec![0, 1], vec![1, 0]]);
    }

    /// Readings that agree on the plan's order are one index: a self-join
    /// whose atoms all bind their columns left to right builds `edge` once.
    #[test]
    fn reads_that_agree_share_one_index() {
        let catalog = counts(vec![("edge", 2, mutual_edges())]);
        let text = "Q(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).";
        let got = both_families(&catalog, text);
        assert_eq!(got, vec![vec![1, 2, 3], vec![1, 3, 2], vec![2, 1, 3]]);

        let query: JoinQuery = text.parse().unwrap();
        let prepared = prepare(&catalog, &query, &planner_of(LexicographicOptimiser)).unwrap();
        assert_eq!(prepared.indexes().len(), 1);
    }

    /// `r(X, X, Y), s(Y, X)`: the filtered `r` and the reversed `s` are both
    /// read in the plan's order, and the equality stays with its columns.
    #[test]
    fn a_selection_on_a_reordered_atom_keeps_its_equality() {
        let r = vec![vec![1, 1, 5], vec![1, 2, 5], vec![2, 2, 6], vec![3, 3, 7]];
        let s = vec![vec![5, 1], vec![6, 2], vec![7, 9]];
        let catalog = counts(vec![("r", 3, r), ("s", 2, s)]);
        let got = both_families(&catalog, "Q(X, Y) :- r(X, X, Y), s(Y, X).");
        assert_eq!(got, vec![vec![1, 5], vec![2, 6]]);
    }

    /// An atom of constants only reads no column: it is a check on how many
    /// rows match. Zero matches means no answers. Otherwise the count
    /// multiplies the answers on a multiset (`HashTrie`) and is clamped to one
    /// on a set (the sorted tries).
    #[test]
    fn an_all_constant_atom_is_a_check() {
        let catalog = counts(vec![
            ("p", 1, vec![vec![1], vec![2]]),
            ("q", 2, vec![vec![1, 2], vec![1, 2], vec![3, 4]]),
        ]);
        let query: JoinQuery = "Q(X) :- p(X), q(c1, c2).".parse().unwrap();
        let mut sorted = sorted_family(&catalog, &query).unwrap();
        sorted.sort();
        assert_eq!(sorted, vec![vec![1], vec![2]]);
        let mut hashed = hash_family(&catalog, &query).unwrap();
        hashed.sort();
        assert_eq!(hashed, vec![vec![1], vec![1], vec![2], vec![2]]);

        let none: JoinQuery = "Q(X) :- p(X), q(c9, c9).".parse().unwrap();
        assert_eq!(sorted_family(&catalog, &none), Ok(Vec::new()));
        assert_eq!(hash_family(&catalog, &none), Ok(Vec::new()));
    }

    /// `Const_`, `Select_` and `Index_` used to be reserved for the predicates
    /// the join synthesised. Indexes now live under their own ids, so a
    /// relation with such a name joins like any other.
    #[test]
    fn names_that_used_to_be_reserved_are_ordinary_relations() {
        let catalog = counts(vec![
            ("Const_c5", 1, vec![vec![5], vec![6]]),
            ("Select_0_r", 2, vec![vec![5, 50], vec![7, 70]]),
            ("Index_1_0_edge", 2, vec![vec![50, 5]]),
        ]);
        let got = both_families(
            &catalog,
            "Q(X, Y) :- Const_c5(X), Select_0_r(X, Y), Index_1_0_edge(Y, X).",
        );
        assert_eq!(got, vec![vec![5, 50]]);
    }
}
