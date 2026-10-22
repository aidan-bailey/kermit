//! One query over a [`Catalog`], in three steps. `prepare` runs once and is not
//! timed: it validates the query, gives each body atom an access, plans over
//! the accesses and decides which indexes the plan reads. `build` and `execute`
//! are what a benchmark times: `build` copies each index out of the catalog's
//! flat tuples, and `execute` joins them.

use {
    super::{validate_query, Catalog, JoinError},
    kermit_algos::{
        distinct_per_column, extract_accesses, plan_indexes, rewrite_placeholders, Access,
        CatalogStats, ColumnOrderPolicy, IndexPlan, IndexSpec, JoinQuery, Planner, QueryPlan,
        RelationStats, StatisticsLevel, ACCESS_PREFIX, INDEX_ID_PREFIX,
    },
    kermit_ds::{Relation, RelationHeader, TupleView, Tuples},
    std::collections::BTreeMap,
};

/// What a [`Prepared`] expects of the catalog it is used with: `prepare`
/// checked it, so a missing relation means another catalog was passed.
const CATALOG_HOLDS: &str =
    "`prepare` checked that the catalog holds every relation the query reads";

/// A query planned over a [`Catalog`]: what [`prepare`] decides, and all that
/// building and executing need besides the catalog itself.
///
/// It holds the executed query (the head as written, and one atom per body atom
/// that binds a variable, named `#0`, `#1`, … after the index it reads, over
/// its variables in plan order), the plan for that query, the index specs, and
/// the constant checks (the atoms with no variable, which only count rows). It
/// borrows nothing, and is meant for the catalog it was prepared over.
#[derive(Clone, Debug)]
pub struct Prepared {
    query: JoinQuery,
    plan: QueryPlan,
    indexes: Vec<IndexSpec>,
    /// The rows `indexes[id]`'s view admits, counted by `prepare`.
    index_rows: Vec<usize>,
    checks: Vec<Access>,
    head_len: usize,
}

impl Prepared {
    /// The executed query: the head as written, and one atom `#<id>` per body
    /// atom that binds a variable.
    pub fn query(&self) -> &JoinQuery { &self.query }

    /// The plan for [`query`](Self::query); it validates against it.
    pub fn plan(&self) -> &QueryPlan { &self.plan }

    /// The indexes the query reads, `indexes()[id]` being `#<id>`. Identical
    /// specs are merged, so an atom list that reads one relation one way
    /// builds it once.
    pub fn indexes(&self) -> &[IndexSpec] { &self.indexes }

    /// The atoms with no variable, in body order. Each only has to count the
    /// rows its constants admit.
    pub fn checks(&self) -> &[Access] { &self.checks }

    /// The number of head terms: the length of the prefix of each joined row
    /// that `execute` lends to its sink.
    pub fn head_len(&self) -> usize { self.head_len }

    /// The distinct relations the query reads: those of its indexes in id
    /// order, then those of its checks, each at its first appearance.
    pub fn relations(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        let read = self
            .indexes
            .iter()
            .map(|spec| spec.relation.as_str())
            .chain(self.checks.iter().map(|check| check.relation.as_str()));
        for name in read {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }
}

/// Validates `query` against `catalog` and plans it: the untimed part of
/// running a query.
///
/// The steps, in order: [`validate_query`]; rewrite each `_` into a fresh
/// variable; extract one access per body atom; refuse a planner that reads more
/// statistics than the catalog gathered; take each access's statistics (an
/// unfiltered access is its relation's, a filtered one is counted by scanning
/// the relation through its filter); plan the query of accesses; and turn that
/// plan into index specs.
///
/// An index is built in plan order, so no stored column order constrains the
/// plan: the statistics are always built under [`ColumnOrderPolicy::Any`],
/// whatever policy `planner` was made with, and `planner` plans over them
/// freely.
///
/// # Errors
///
/// The [`JoinError`] [`validate_query`] would return, or
/// [`JoinError::MissingStatistics`] when `planner` reads statistics the catalog
/// does not hold; in both cases nothing has been planned.
pub fn prepare(
    catalog: &Catalog, query: &JoinQuery, planner: &Planner,
) -> Result<Prepared, JoinError> {
    // The policy argument goes when `ColumnOrderPolicy` does.
    validate_query(query, catalog, ColumnOrderPolicy::Any)?;

    let rewritten = rewrite_placeholders(query.clone());
    let access_query = extract_accesses(&rewritten)?;

    // The plan comes from the catalog's statistics, so the optimiser must not
    // read more than the catalog gathered when it was built.
    let required = planner.required_statistics();
    if required > catalog.level() {
        return Err(JoinError::MissingStatistics {
            required,
            available: catalog.level(),
        });
    }

    let access_stats: BTreeMap<String, RelationStats> = access_query
        .accesses
        .iter()
        .enumerate()
        .map(|(k, access)| {
            let id = format!("{ACCESS_PREFIX}{k}");
            (id, access_statistics(catalog, access, required))
        })
        .collect();
    let stats = CatalogStats::for_query(&access_query.query, ColumnOrderPolicy::Any, |id| {
        access_stats.get(id).cloned()
    });
    let ordering = planner.plan(&access_query.query, &stats);

    let IndexPlan {
        query: executed,
        plan,
        indexes,
    } = plan_indexes(&access_query, &ordering);

    // Counted here, untimed: `build` would otherwise scan a filtered relation
    // once more only to learn how many rows its copy holds.
    let index_rows: Vec<usize> = indexes
        .iter()
        .map(|spec| spec.view(tuples_of(catalog, &spec.relation)).count())
        .collect();

    Ok(Prepared {
        query: executed,
        plan,
        indexes,
        index_rows,
        checks: access_query.checks,
        head_len: query.head.terms.len(),
    })
}

/// The rows of `relation` in `catalog`.
fn tuples_of<'c>(catalog: &'c Catalog, relation: &str) -> &'c Tuples {
    catalog.tuples(relation).expect(CATALOG_HOLDS)
}

/// The statistics the planner reads for `access`. An access that keeps every
/// row and column is its relation's catalog entry. Any other is scanned through
/// its view: the rows it admits, and, when `level` asks for them, the distinct
/// values in each column it keeps.
fn access_statistics(catalog: &Catalog, access: &Access, level: StatisticsLevel) -> RelationStats {
    let tuples = tuples_of(catalog, &access.relation);
    let view = TupleView::new(tuples, access.filter.clone(), access.columns.clone());
    if view.is_identity() {
        return catalog
            .statistics(&access.relation)
            .expect(CATALOG_HOLDS)
            .clone();
    }
    let stats = RelationStats::new(view.count(), view.arity());
    if level < StatisticsLevel::ColumnDistinct {
        return stats;
    }
    stats.with_column_distinct(distinct_per_column(view.arity(), |visit| {
        view.for_each(visit)
    }))
}

/// The header of `spec`'s index: named for the relation, with the attributes of
/// `base` at the spec's columns, or positional (at the index's arity) when
/// `base` is.
fn spec_header(spec: &IndexSpec, base: &RelationHeader) -> RelationHeader {
    if base.attrs().is_empty() {
        RelationHeader::new_positional(&spec.relation, spec.columns.len())
    } else {
        let attrs = spec
            .columns
            .iter()
            .map(|&column| base.attrs()[column].clone())
            .collect();
        RelationHeader::new(&spec.relation, attrs)
    }
}

/// The indexes of one prepared query, made by [`build`] or [`build_with`]: one
/// structure per [`IndexSpec`], in id order, and what the constant checks
/// decided.
///
/// The indexes hold copies of their rows and borrow nothing from the catalog,
/// so a benchmark can drop them after its timers stop.
pub struct Indexes<R> {
    /// The specs the indexes were built from, in id order.
    specs: Vec<IndexSpec>,
    /// `relations[id]` is index `#<id>`.
    relations: Vec<R>,
    multiplicity: usize,
    rows: usize,
}

impl<R> Indexes<R> {
    /// The index named `id` (`#0`, `#1`, …), if there is one. Only the exact
    /// spelling names an index.
    pub fn get(&self, id: &str) -> Option<&R> {
        let digits = id.strip_prefix(INDEX_ID_PREFIX)?;
        // `parse` alone would also take `#01` and `#+1`, which no id is spelled
        // as.
        let padded = digits.len() > 1 && digits.starts_with('0');
        if padded || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        self.relations.get(digits.parse::<usize>().ok()?)
    }

    /// Each index with the spec it was built from, in id order.
    pub fn iter(&self) -> impl Iterator<Item = (&IndexSpec, &R)> {
        self.specs.iter().zip(&self.relations)
    }

    /// How many times `execute` emits each answer: the product of the constant
    /// checks' counts, each clamped to 1 unless the structure keeps
    /// duplicates ([`Relation::KEEPS_DUPLICATES`]). Zero when a check
    /// admits no row.
    pub fn multiplicity(&self) -> usize { self.multiplicity }

    /// The rows the build copied: the sum of the index views' counts, taken
    /// when the query was prepared.
    pub fn rows(&self) -> usize { self.rows }
}

/// Builds the indexes of `prepared` from `catalog`, constructing each with
/// `construct`: the timed build.
///
/// Each spec becomes a [`TupleView`] over its relation's flat tuples, handed
/// with the index's header to `construct` (normally [`Relation::from_view`], as
/// [`build`] does; the binary's families pass their own to carry a config and a
/// build mode). Each constant check counts the rows its constants admit, and
/// the counts multiply into [`multiplicity`](Indexes::multiplicity).
///
/// # Panics
///
/// If `prepared` was made over another catalog and a relation it reads is
/// missing. In debug builds, also if the catalog's rows changed since `prepare`
/// counted them.
pub fn build_with<R: Relation>(
    catalog: &Catalog, prepared: &Prepared,
    mut construct: impl FnMut(RelationHeader, TupleView<'_>) -> R,
) -> Indexes<R> {
    // A check keeps no column: it only counts the rows its filter admits. A
    // structure that keeps duplicates answers once per row; one that does
    // not answers once.
    let mut multiplicity = 1usize;
    for check in &prepared.checks {
        let tuples = tuples_of(catalog, &check.relation);
        let count = TupleView::new(tuples, check.filter.clone(), Vec::new()).count();
        let copies = if R::KEEPS_DUPLICATES {
            count
        } else {
            count.min(1)
        };
        multiplicity = multiplicity.saturating_mul(copies);
    }

    let relations: Vec<R> = prepared
        .indexes
        .iter()
        .zip(&prepared.index_rows)
        .map(|(spec, &rows)| {
            let base = catalog.header(&spec.relation).expect(CATALOG_HOLDS);
            let view = spec.view(tuples_of(catalog, &spec.relation));
            debug_assert_eq!(view.count(), rows, "the catalog changed since `prepare`");
            construct(spec_header(spec, base), view)
        })
        .collect();

    Indexes {
        specs: prepared.indexes.clone(),
        relations,
        multiplicity,
        rows: prepared.index_rows.iter().sum(),
    }
}

/// [`build_with`] constructing each index with [`Relation::from_view`].
///
/// # Panics
///
/// As [`build_with`].
pub fn build<R: Relation>(catalog: &Catalog, prepared: &Prepared) -> Indexes<R> {
    build_with(catalog, prepared, R::from_view)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_algos::{analyse, LexicographicOptimiser, QueryOptimiser, RewriteError},
        kermit_ds::{Cardinality, Filter, HashTrie, RelationHeader, TreeTrie},
        std::{cell::RefCell, rc::Rc},
    };

    /// A catalog of positional relations, each `(name, arity, rows)`, gathered
    /// at `level`.
    fn catalog_at(
        level: StatisticsLevel, relations: Vec<(&str, usize, Vec<Vec<usize>>)>,
    ) -> Catalog {
        let relations = relations
            .into_iter()
            .map(|(name, arity, rows)| {
                let mut tuples = Tuples::new(arity);
                for row in &rows {
                    tuples.push(row);
                }
                (RelationHeader::new_positional(name, arity), tuples)
            })
            .collect();
        Catalog::new(relations, level)
    }

    /// [`catalog_at`] at tuple counts, which is all the default optimiser
    /// reads.
    fn catalog(relations: Vec<(&str, usize, Vec<Vec<usize>>)>) -> Catalog {
        catalog_at(StatisticsLevel::TupleCounts, relations)
    }

    fn parse(text: &str) -> JoinQuery { text.parse().expect("a well-formed query") }

    /// A planner for `optimiser` under the only policy `prepare` plans with.
    fn planner_of(optimiser: impl QueryOptimiser + 'static) -> Planner {
        Planner::new(Box::new(optimiser), ColumnOrderPolicy::Any)
    }

    /// Plans in canonical variable order, so a test can read the plan off the
    /// query.
    fn lexicographic() -> Planner { planner_of(LexicographicOptimiser) }

    fn prepared_for(catalog: &Catalog, text: &str) -> Prepared {
        prepare(catalog, &parse(text), &lexicographic()).expect("a valid query")
    }

    fn spec(relation: &str, filter: Filter, columns: Vec<usize>) -> IndexSpec {
        IndexSpec {
            relation: relation.into(),
            filter,
            columns,
        }
    }

    #[test]
    fn a_constant_filters_the_index() {
        let c = catalog(vec![("p", 2, vec![vec![1, 10], vec![2, 20], vec![3, 30]])]);
        let prepared = prepared_for(&c, "Q(X) :- p(X, c10).");
        assert_eq!(prepared.indexes().to_vec(), vec![spec(
            "p",
            Filter::new(vec![(1, 10)], vec![]),
            vec![0]
        )]);
        assert!(prepared.checks().is_empty());
        assert_eq!(prepared.relations(), vec!["p"]);
        assert_eq!(prepared.head_len(), 1);
        assert_eq!(prepared.query().to_string(), "Q(X) :- #0(X).");
        assert_eq!(prepared.plan().variable_ordering, vec![0]);
    }

    #[test]
    fn a_repeated_variable_becomes_an_equality() {
        let c = catalog(vec![("r", 2, vec![
            vec![1, 1],
            vec![1, 2],
            vec![2, 3],
            vec![3, 3],
        ])]);
        let prepared = prepared_for(&c, "Q(X) :- r(X, X).");
        assert_eq!(prepared.indexes().to_vec(), vec![spec(
            "r",
            Filter::new(vec![], vec![(0, 1)]),
            vec![0]
        )]);
        assert_eq!(prepared.query().to_string(), "Q(X) :- #0(X).");
    }

    #[test]
    fn a_constant_and_a_repeat_share_one_filter() {
        let c = catalog(vec![("t", 3, vec![vec![1, 5, 1], vec![1, 5, 2], vec![
            2, 6, 2,
        ]])]);
        let prepared = prepared_for(&c, "Q(X) :- t(X, c5, X).");
        assert_eq!(prepared.indexes().to_vec(), vec![spec(
            "t",
            Filter::new(vec![(1, 5)], vec![(0, 2)]),
            vec![0]
        )]);
    }

    /// `r(X, X)` is a filtered read of `r` and `r(X, Y)` an unfiltered one: two
    /// indexes.
    #[test]
    fn mixed_occurrences_of_one_relation_get_two_indexes() {
        let c = catalog(vec![("r", 2, vec![
            vec![1, 1],
            vec![1, 2],
            vec![2, 3],
            vec![3, 3],
        ])]);
        let prepared = prepared_for(&c, "Q(X, Y) :- r(X, X), r(X, Y).");
        assert_eq!(prepared.indexes().to_vec(), vec![
            spec("r", Filter::new(vec![], vec![(0, 1)]), vec![0]),
            spec("r", Filter::none(), vec![0, 1]),
        ]);
        assert_eq!(prepared.query().to_string(), "Q(X, Y) :- #0(X), #1(X, Y).");
        assert_eq!(prepared.relations(), vec!["r"]);
    }

    /// A `_` is an unused named variable, so its column is kept. In the middle
    /// the plan binds `Z` before the placeholder's variable, so the index
    /// reads columns 0, 2, 1.
    #[test]
    fn a_placeholder_is_a_kept_column() {
        let c = catalog(vec![
            ("r", 2, vec![vec![1, 2]]),
            ("t", 3, vec![vec![1, 2, 3]]),
        ]);
        let trailing = prepared_for(&c, "Q(X) :- r(X, _).");
        assert_eq!(trailing.indexes().to_vec(), vec![spec(
            "r",
            Filter::none(),
            vec![0, 1]
        )]);
        let middle = prepared_for(&c, "Q(X, Z) :- t(X, _, Z).");
        assert_eq!(middle.indexes().to_vec(), vec![spec(
            "t",
            Filter::none(),
            vec![0, 2, 1]
        )]);
        assert_eq!(middle.head_len(), 2);
    }

    /// Three reads of `e`, all in the same column order: one index, read three
    /// times.
    #[test]
    fn a_self_join_reads_one_merged_index() {
        let c = catalog(vec![("e", 2, vec![vec![1, 2], vec![2, 3], vec![1, 3]])]);
        let prepared = prepared_for(&c, "Q(X, Y, Z) :- e(X, Y), e(Y, Z), e(X, Z).");
        assert_eq!(prepared.indexes().to_vec(), vec![spec(
            "e",
            Filter::none(),
            vec![0, 1]
        )]);
        assert_eq!(
            prepared.query().to_string(),
            "Q(X, Y, Z) :- #0(X, Y), #0(Y, Z), #0(X, Z)."
        );
        assert_eq!(prepared.relations(), vec!["e"]);
        assert_eq!(prepared.plan().validate(&analyse(prepared.query())), Ok(()));
    }

    /// Reading a relation against its stored column order needs no copy
    /// protocol: the index is simply built reversed, and `edge(X, Y),
    /// edge(Y, X)` gets two.
    #[test]
    fn a_reversed_atom_gets_its_own_index() {
        let c = catalog(vec![("edge", 2, vec![vec![1, 2], vec![3, 4]])]);
        let reversed = prepared_for(&c, "Q(X, Y) :- edge(Y, X).");
        assert_eq!(reversed.indexes().to_vec(), vec![spec(
            "edge",
            Filter::none(),
            vec![1, 0]
        )]);
        assert_eq!(reversed.query().to_string(), "Q(X, Y) :- #0(X, Y).");

        let cycle = prepared_for(&c, "Q(X, Y) :- edge(X, Y), edge(Y, X).");
        assert_eq!(cycle.indexes().to_vec(), vec![
            spec("edge", Filter::none(), vec![0, 1]),
            spec("edge", Filter::none(), vec![1, 0]),
        ]);
        assert_eq!(cycle.query().to_string(), "Q(X, Y) :- #0(X, Y), #1(X, Y).");
    }

    /// An atom with no variable needs no index: it is a check, and its relation
    /// is still read.
    #[test]
    fn all_constant_atoms_become_checks() {
        let c = catalog(vec![
            ("r", 1, vec![vec![1], vec![2]]),
            ("s", 2, vec![vec![7, 8], vec![9, 9]]),
        ]);
        let prepared = prepared_for(&c, "Q(X) :- r(X), s(c7, c8).");
        assert_eq!(prepared.indexes().to_vec(), vec![spec(
            "r",
            Filter::none(),
            vec![0]
        )]);
        assert_eq!(prepared.checks().to_vec(), vec![Access {
            relation: "s".into(),
            filter: Filter::new(vec![(0, 7), (1, 8)], vec![]),
            columns: vec![],
            variables: vec![],
        }]);
        assert_eq!(prepared.relations(), vec!["r", "s"]);
        assert_eq!(prepared.query().to_string(), "Q(X) :- #0(X).");
        assert_eq!(prepared.head_len(), 1);
    }

    #[test]
    fn a_malformed_query_is_refused_with_validations_error() {
        let c = catalog(vec![("edge", 2, vec![vec![1, 2]])]);
        let refused = |text: &str| prepare(&c, &parse(text), &lexicographic()).unwrap_err();
        assert_eq!(refused("Q(X) :- missing(X)."), JoinError::UnknownRelation {
            relation: "missing".into(),
            known: vec!["edge".into()],
        });
        assert_eq!(refused("Q(X) :- edge(X)."), JoinError::ArityMismatch {
            atom: "edge(X)".into(),
            relation: "edge".into(),
            terms: 1,
            arity: 2,
        });
        assert_eq!(
            refused("Q(X) :- edge(X, cfoo)."),
            JoinError::MalformedConstant(RewriteError::BadAtom("cfoo".into()))
        );
        assert_eq!(
            refused("Q(Z) :- edge(X, Y)."),
            JoinError::UnboundHeadVariable {
                variable: "Z".into(),
            }
        );
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

    /// A query that fails validation reports that error, not
    /// `MissingStatistics`: the statistics check runs after validation.
    #[test]
    fn validation_errors_win_over_missing_statistics() {
        let c = catalog(vec![("edge", 2, vec![vec![1, 2]])]);
        let refused = prepare(&c, &parse("Q(X) :- missing(X)."), &planner_of(NeedsColumns));
        assert_eq!(refused.unwrap_err(), JoinError::UnknownRelation {
            relation: "missing".into(),
            known: vec!["edge".into()],
        });
    }

    /// An optimiser that reads more than the catalog gathered is refused, and
    /// the same relations gathered at its level are prepared.
    #[test]
    fn an_optimiser_reading_ungathered_statistics_is_refused() {
        let relations = || {
            vec![
                ("first", 1, vec![vec![1], vec![2], vec![3]]),
                ("second", 1, vec![vec![2], vec![3], vec![4]]),
            ]
        };
        let query = parse("Q(X) :- first(X), second(X).");
        let planner = planner_of(NeedsColumns);
        let refused = prepare(&catalog(relations()), &query, &planner);
        assert_eq!(refused.unwrap_err(), JoinError::MissingStatistics {
            required: StatisticsLevel::ColumnDistinct,
            available: StatisticsLevel::TupleCounts,
        });
        let analysed = catalog_at(StatisticsLevel::ColumnDistinct, relations());
        assert!(prepare(&analysed, &query, &planner).is_ok());
    }

    /// Plans like `lexicographic`, records the query and statistics it is
    /// handed, and declares that it reads `level`. Clones share the record,
    /// so a test keeps one while the planner owns the other.
    #[derive(Clone)]
    struct StatsSpy {
        level: StatisticsLevel,
        seen: Rc<RefCell<Option<(JoinQuery, CatalogStats)>>>,
    }

    impl StatsSpy {
        fn new(level: StatisticsLevel) -> Self {
            StatsSpy {
                level,
                seen: Rc::default(),
            }
        }

        /// What the optimiser was handed by the last `prepare`.
        fn taken(&self) -> (JoinQuery, CatalogStats) {
            self.seen
                .borrow_mut()
                .take()
                .expect("the optimiser planned")
        }
    }

    impl QueryOptimiser for StatsSpy {
        fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
            *self.seen.borrow_mut() = Some((query.clone(), stats.clone()));
            LexicographicOptimiser.plan(query, stats)
        }

        fn required_statistics(&self) -> StatisticsLevel { self.level }
    }

    /// Three relations for [`STATISTICS_QUERY`]. `r` has two diagonal rows, `s`
    /// has three rows with 5 in its second column, and `u` is read whole.
    fn statistics_relations() -> Vec<(&'static str, usize, Vec<Vec<usize>>)> {
        vec![
            ("r", 2, vec![vec![1, 1], vec![1, 2], vec![2, 3], vec![3, 3]]),
            ("s", 3, vec![
                vec![1, 5, 7],
                vec![1, 5, 8],
                vec![3, 5, 9],
                vec![3, 6, 7],
                vec![4, 6, 6],
            ]),
            ("u", 2, vec![vec![1, 2], vec![1, 3], vec![2, 3], vec![2, 4]]),
        ]
    }

    const STATISTICS_QUERY: &str = "Q(X, Y) :- r(X, X), s(X, c5, W), u(X, Y).";

    /// The planner plans the query of accesses and reads statistics keyed by
    /// access id: a filtered access is counted through its filter, an
    /// unfiltered one is its relation's.
    #[test]
    fn the_planner_reads_statistics_of_the_accesses() {
        let c = catalog_at(StatisticsLevel::ColumnDistinct, statistics_relations());
        let spy = StatsSpy::new(StatisticsLevel::ColumnDistinct);
        prepare(&c, &parse(STATISTICS_QUERY), &planner_of(spy.clone())).unwrap();
        let (planned, stats) = spy.taken();

        assert_eq!(planned.to_string(), "Q(X, Y) :- @0(X), @1(X, W), @2(X, Y).");
        // r(X, X): the rows (1, 1) and (3, 3), kept column 0.
        assert_eq!(stats.tuples("@0"), Some(2));
        assert_eq!(stats.distinct("@0", 0), Some(2));
        // s(X, c5, W): the rows (1, 5, 7), (1, 5, 8) and (3, 5, 9), kept
        // columns 0 and 2.
        assert_eq!(stats.tuples("@1"), Some(3));
        assert_eq!(
            (stats.distinct("@1", 0), stats.distinct("@1", 1)),
            (Some(2), Some(3))
        );
        // u(X, Y): unfiltered, so the catalog's own statistics.
        assert_eq!(stats.tuples("@2"), Some(4));
        assert_eq!(
            (stats.distinct("@2", 0), stats.distinct("@2", 1)),
            (Some(2), Some(3))
        );
        // Nothing is keyed by the relations' own names.
        assert_eq!(stats.tuples("r"), None);
    }

    /// An optimiser that reads only tuple counts gets exact counts of the
    /// filtered accesses and no distinct counts, so no filtered scan pays
    /// for what nobody reads.
    #[test]
    fn a_planner_reading_only_counts_gets_counts_of_filtered_accesses() {
        let c = catalog(statistics_relations());
        let spy = StatsSpy::new(StatisticsLevel::TupleCounts);
        prepare(&c, &parse(STATISTICS_QUERY), &planner_of(spy.clone())).unwrap();
        let (_, stats) = spy.taken();
        let counts = (stats.tuples("@0"), stats.tuples("@1"), stats.tuples("@2"));
        assert_eq!(counts, (Some(2), Some(3), Some(4)));
        for id in ["@0", "@1", "@2"] {
            assert_eq!(stats.distinct(id, 0), None, "{id}");
        }
    }

    /// How many tuples each index of `indexes` stores, in id order.
    fn stored<R: Cardinality>(indexes: &Indexes<R>) -> Vec<usize> {
        indexes
            .iter()
            .map(|(_, index)| index.tuple_count())
            .collect()
    }

    /// Each index is its relation read through the spec's filter and columns:
    /// `r`'s diagonal (1, 1) and (3, 3); `s(X, c5, W)`'s (1, 7), (1, 8) and
    /// (3, 9); all four rows of `u`.
    #[test]
    fn build_copies_each_index_through_its_filter_and_columns() {
        let c = catalog(statistics_relations());
        let prepared = prepared_for(&c, STATISTICS_QUERY);

        let sorted = build::<TreeTrie>(&c, &prepared);
        let relations: Vec<&str> = sorted
            .iter()
            .map(|(spec, _)| spec.relation.as_str())
            .collect();
        assert_eq!(relations, vec!["r", "s", "u"]);
        assert_eq!(stored(&sorted), vec![2, 3, 4]);
        assert_eq!(sorted.rows(), 9);
        assert_eq!(sorted.multiplicity(), 1);

        let hashed = build::<HashTrie>(&c, &prepared);
        assert_eq!(stored(&hashed), vec![2, 3, 4]);
        assert_eq!(hashed.rows(), 9);
    }

    /// The constructor gets the index's header (named for the relation,
    /// attributes in index order) and the view that `IndexSpec` describes.
    #[test]
    fn build_with_hands_the_constructor_each_header_and_view() {
        let mut tuples = Tuples::new(2);
        tuples.push(&[1, 2]);
        tuples.push(&[3, 4]);
        let header = RelationHeader::new("edge", vec!["src".into(), "dst".into()]);
        let c = Catalog::new(vec![(header, tuples)], StatisticsLevel::TupleCounts);
        let prepared = prepared_for(&c, "Q(X, Y) :- edge(Y, X).");

        let mut seen = Vec::new();
        let indexes: Indexes<TreeTrie> = build_with(&c, &prepared, |header, view| {
            seen.push((header.clone(), view.columns().to_vec(), view.count()));
            <TreeTrie as Relation>::from_view(header, view)
        });

        let reversed = RelationHeader::new("edge", vec!["dst".into(), "src".into()]);
        assert_eq!(seen, vec![(reversed, vec![1, 0], 2)]);
        assert_eq!(indexes.rows(), 2);
    }

    /// The header keeps the relation's name and takes the attributes at the
    /// spec's columns; a positional header stays positional, at the index's
    /// arity.
    #[test]
    fn an_index_header_follows_the_spec_columns() {
        let index = spec("edge", Filter::none(), vec![1]);
        let named = RelationHeader::new("edge", vec!["src".into(), "dst".into()]);
        assert_eq!(
            spec_header(&index, &named),
            RelationHeader::new("edge", vec!["dst".into()])
        );
        let positional = RelationHeader::new_positional("edge", 2);
        assert_eq!(
            spec_header(&index, &positional),
            RelationHeader::new_positional("edge", 1)
        );
    }

    /// Only the spelling `#<k>` names index `k`.
    #[test]
    fn an_index_is_found_only_by_its_exact_id() {
        let c = catalog(vec![("p", 1, vec![vec![1]])]);
        let indexes = build::<TreeTrie>(&c, &prepared_for(&c, "Q(X) :- p(X)."));
        assert!(indexes.get("#0").is_some());
        for id in ["#1", "#00", "#+0", "#", "@0", "0", "#18446744073709551616"] {
            assert!(indexes.get(id).is_none(), "{id}");
        }
    }

    /// An all-constant atom counts the rows its constants admit. Hash tries
    /// keep duplicates, so they multiply by the count; the sorted tries do
    /// not, so they clamp it to one. A count of zero empties the product on
    /// both.
    #[test]
    fn checks_multiply_on_hash_tries_only() {
        let c = catalog(vec![
            ("r", 1, vec![vec![1], vec![2]]),
            ("s", 2, vec![vec![7, 8], vec![7, 8], vec![9, 9]]),
            ("t", 1, vec![vec![5], vec![5], vec![5]]),
        ]);
        let multiplicity = |text: &str| {
            let prepared = prepared_for(&c, text);
            (
                build::<TreeTrie>(&c, &prepared).multiplicity(),
                build::<HashTrie>(&c, &prepared).multiplicity(),
            )
        };
        assert_eq!(multiplicity("Q(X) :- r(X)."), (1, 1));
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8)."), (1, 2));
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8), t(c5)."), (1, 6));
        assert_eq!(multiplicity("Q(X) :- r(X), s(c7, c8), s(c7, c9)."), (0, 0));
    }

    #[test]
    fn an_empty_relation_builds_an_empty_index() {
        let c = catalog(vec![("e", 2, vec![])]);
        let indexes = build::<TreeTrie>(&c, &prepared_for(&c, "Q(X) :- e(X, Y)."));
        assert_eq!(indexes.rows(), 0);
        assert_eq!(stored(&indexes), vec![0]);
    }
}
