//! Engine cells: an external [`Engine`] (DuckDB, Nemo) run as one more cell of
//! a `bench run`, `bench join` or `bench probe` sweep.
//!
//! [`EngineSubject`] is the [`Subject`] the measure layer times: `build` is one
//! timed engine build, `exercise` one streamed count, `materialize` one stored
//! result. [`EngineCell`] is the [`Cell`] the per-query loop drives. Both are
//! generic in the engine, so adding an engine adds no code here. The dispatch
//! in `run.rs` prints [`skip_notes`] once per cell and benchmark;
//! `run_benchmark` filters the requested metrics through [`Cell::supports`].
//!
//! Every engine method is fallible, but a timed body cannot return an error. So
//! [`EngineCell::preflight`] runs the whole path once (the count only when
//! asked), untimed, inside [`Cell::subject`], and turns any error into the
//! cell's error; the subject keeps the preflight's load. The timed bodies then
//! `expect`, because an error that first appears after a clean preflight is a
//! broken invariant.

use {
    super::{
        cell::Cell,
        measure::{Materialized, Subject},
        Metric,
    },
    crate::bench_report::MetadataLine,
    anyhow::Context,
    kermit::{
        db::{check_multiplicity, Catalog, Prepared},
        engine::{Engine, EngineError},
    },
    kermit_ds::TrieLevels,
    std::collections::BTreeMap,
};

/// Unwraps an engine result that a clean preflight has already seen succeed.
fn expect_clean<T>(result: Result<T, EngineError>, step: &str) -> T {
    result.unwrap_or_else(|error| panic!("{step} failed after a clean preflight: {error}"))
}

/// A prepared query on one engine: the query's relations loaded into the
/// engine, untimed.
///
/// `loaded` is `None` for a query made only of checks (all-constant atoms),
/// which never reaches the engine.
pub(crate) struct EngineSubject<'a, E: Engine + 'static> {
    loaded: Option<E::Loaded>,
    catalog: &'a Catalog,
    prepared: &'a Prepared,
}

impl<E: Engine + 'static> EngineSubject<'_, E> {
    /// The running engine's version, for the report's `engine_version` axis.
    /// `None` when the query never loaded the engine.
    fn version(&self) -> Option<String> { self.loaded.as_ref().map(E::version) }
}

/// What one timed build leaves: the engine's built state, and the product of
/// the all-constant atoms' counts (`kermit::db::check_multiplicity`), the
/// lifecycle's rule.
pub(crate) struct EngineBuilt<'s, E: Engine + 'static> {
    /// `None` for a query of checks only, and for a zero multiplicity (no
    /// answers).
    engine: Option<E::Built<'s>>,
    multiplicity: usize,
}

impl<E: Engine + 'static> Subject for EngineSubject<'_, E> {
    type Built<'s>
        = EngineBuilt<'s, E>
    where
        Self: 's;

    /// `stream` does not change the built state; Nemo, which would change it,
    /// has no `stream`.
    const EXERCISE_MUTATES: bool = false;
    const MATERIALIZE_MUTATES: bool = E::MATERIALIZE_MUTATES;
    const STREAMS: bool = E::STREAMS;

    /// One timed build. The all-constant atoms' multiplicity is computed here,
    /// inside the timed body, as `kermit::db::build_with` computes it inside
    /// the timed build of a kermit cell.
    fn build(&self) -> EngineBuilt<'_, E> {
        let multiplicity = check_multiplicity(self.catalog, self.prepared, E::KEEPS_DUPLICATES);
        let engine = match &self.loaded {
            | Some(loaded) if multiplicity > 0 => Some(expect_clean(
                E::build(loaded, self.catalog, self.prepared),
                "build",
            )),
            | _ => None,
        };
        EngineBuilt {
            engine,
            multiplicity,
        }
    }

    /// The engine's own space as one figure, or none: an engine's bytes are not
    /// per index.
    fn heap_bytes(&self, built: &EngineBuilt<'_, E>) -> Vec<usize> {
        built
            .engine
            .as_ref()
            .and_then(E::space)
            .map(|bytes| vec![bytes])
            .unwrap_or_default()
    }

    /// The streamed row count, each row repeated `multiplicity` times as
    /// `kermit::db::execute` repeats it. A query of checks has the one empty
    /// row, `multiplicity` times; a zero multiplicity has none.
    fn exercise(&self, built: &EngineBuilt<'_, E>) -> u64 {
        assert!(
            E::STREAMS,
            "{} does not stream: exercise is only called on a streaming engine",
            E::NAME
        );
        let multiplicity = u64::try_from(built.multiplicity).unwrap_or(u64::MAX);
        match &built.engine {
            | Some(engine) => {
                expect_clean(E::stream(engine), "stream").saturating_mul(multiplicity)
            },
            | None => multiplicity,
        }
    }

    /// The distinct full assignments, and the engine's sorted batch as `kept`
    /// when it copies its rows into `Tuples` (DuckDB), which Criterion frees
    /// after the timer. The multiplicity repeats equal rows, which
    /// deduplication removes, so it only decides whether there are any; a query
    /// of checks has the one empty row.
    fn materialize(&self, built: &EngineBuilt<'_, E>) -> Option<Materialized> {
        let (rows, kept) = match (&built.engine, built.multiplicity) {
            | (_, 0) => (0, None),
            | (Some(engine), _) => expect_clean(E::materialize(engine), "materialize"),
            | (None, _) => (1, None),
        };
        Some(Materialized {
            rows,
            kept,
        })
    }
}

/// One engine, with its settings, as a cell of a sweep.
pub(crate) struct EngineCell<E: Engine> {
    pub(crate) settings: E::Settings,
}

impl<E: Engine + 'static> EngineCell<E> {
    /// The preflight of [`Cell::subject`]: every fallible engine step once,
    /// so that the timed bodies may `expect`. Returns the loaded state for
    /// the subject to keep.
    fn preflight(
        &self, catalog: &Catalog, prepared: &Prepared, count: bool,
    ) -> anyhow::Result<E::Loaded> {
        let label = self.label();
        E::supports(prepared).map_err(|unsupported| {
            anyhow::anyhow!(
                "{label} cannot run query '{}': {}",
                prepared.query(),
                unsupported.reason
            )
        })?;
        let context = || format!("{label} failed on query '{}'", prepared.query());
        let loaded = E::load(&self.settings, catalog, prepared).with_context(context)?;
        {
            let built = E::build(&loaded, catalog, prepared).with_context(context)?;
            if count {
                if E::STREAMS {
                    E::stream(&built).with_context(context)?;
                } else {
                    E::materialize(&built).with_context(context)?;
                }
            }
        }
        Ok(loaded)
    }
}

impl<E: Engine + 'static> Cell for EngineCell<E> {
    type Subject<'a>
        = EngineSubject<'a, E>
    where
        Self: 'a;

    /// An engine's bytes are one figure for the whole query, so the report has
    /// no `index` lines.
    const PER_INDEX_BYTES: bool = false;

    fn label(&self) -> String { E::label(&self.settings) }

    fn metadata(&self) -> Vec<MetadataLine> {
        let mut lines = vec![MetadataLine::new("system", E::NAME)];
        if let Some(threads) = E::threads(&self.settings) {
            lines.push(MetadataLine::new("engine threads", threads));
        }
        lines
    }

    /// `system` and, for an engine with a thread count, `engine_threads`. The
    /// engine row omits `data_structure`, `algorithm` and every `ds_*` axis;
    /// `engine_version` comes from the subject ([`Cell::subject_axes`]).
    fn axes(&self) -> BTreeMap<String, serde_json::Value> {
        let mut axes = BTreeMap::from([("system".to_string(), serde_json::json!(E::NAME))]);
        if let Some(threads) = E::threads(&self.settings) {
            axes.insert("engine_threads".to_string(), serde_json::json!(threads));
        }
        axes
    }

    fn group_segment(&self) -> String { E::label(&self.settings) }

    /// Engines read the flat tuples through filters, so they share the sorted
    /// cells' prepared queries.
    fn trie_levels(&self) -> TrieLevels { TrieLevels::All }

    fn supports(&self, metric: Metric) -> bool {
        match metric {
            | Metric::Build | Metric::Materialize => true,
            | Metric::Iteration | Metric::EndToEnd => E::STREAMS,
            | Metric::Space => E::HAS_SPACE,
        }
    }

    /// The preflight, untimed: support, then the one load the subject keeps,
    /// then a trial build and, when `count` is true, one count on it
    /// (`stream`, or else `materialize`). The trial build is dropped before
    /// the describe step builds. `count` is false only when the memory guard
    /// skips `materialize` for an engine that does not stream; `run_benchmark`
    /// always passes true for a streaming one. A query of checks never reaches
    /// the engine, so it loads nothing and needs no check.
    fn subject<'a>(
        &'a self, catalog: &'a Catalog, prepared: &'a Prepared, count: bool,
    ) -> anyhow::Result<EngineSubject<'a, E>> {
        let loaded = if prepared.indexes().is_empty() {
            None
        } else {
            Some(self.preflight(catalog, prepared, count)?)
        };
        Ok(EngineSubject {
            loaded,
            catalog,
            prepared,
        })
    }

    fn subject_axes(&self, subject: &EngineSubject<'_, E>) -> BTreeMap<String, serde_json::Value> {
        subject
            .version()
            .map(|version| {
                BTreeMap::from([("engine_version".to_string(), serde_json::json!(version))])
            })
            .unwrap_or_default()
    }
}

/// The stderr notes for the metrics a cell cannot measure and the run asked
/// for: `iteration` and `end-to-end` of an engine that does not stream, and
/// `space` of one that reports none. A probe (`single_run`) asks for a single
/// `iteration` run without naming it. The measure layer acts on the
/// same fact: `Described::measure` skips `iteration` and `end_to_end` for
/// a non-streaming subject, and `Described::materialize_once` /
/// `Described::materialize_time` give it its single-run and timed
/// `materialize`.
pub(crate) fn skip_notes<C: Cell>(cell: &C, metrics: &[Metric], single_run: bool) -> Vec<String> {
    let asked =
        |metric: Metric| metrics.contains(&metric) || (single_run && metric == Metric::Iteration);
    let label = cell.label();
    let mut notes = Vec::new();
    if (asked(Metric::Iteration) || asked(Metric::EndToEnd))
        && !(cell.supports(Metric::Iteration) && cell.supports(Metric::EndToEnd))
    {
        notes.push(format!(
            "note: {label} does not stream, so it skips iteration and end-to-end"
        ));
    }
    if asked(Metric::Space) && !cell.supports(Metric::Space) {
        notes.push(format!("note: {label} reports no space"));
    }
    notes
}

/// A scripted [`Engine`] for the unit tests: no database, fixed answers. `S` is
/// whether it streams, `P` whether it reports space and `K` whether it keeps
/// duplicates, so `Fake<true, false, true>` is shaped like DuckDB and
/// `Fake<false, true, false>` like Nemo.
#[cfg(test)]
pub(crate) mod test_support {
    use {
        kermit::{
            db::{Catalog, Prepared},
            engine::{Engine, EngineError, Unsupported},
        },
        kermit_ds::Tuples,
    };

    thread_local! {
        static COUNTED: std::cell::RefCell<Vec<&'static str>> =
            const { std::cell::RefCell::new(Vec::new()) };
        static LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn counted(call: &'static str) { COUNTED.with(|counted| counted.borrow_mut().push(call)); }

    /// The counting calls (`stream`, `materialize`) the fakes of this test's
    /// thread have made since the last call, in order.
    pub(crate) fn take_counted() -> Vec<&'static str> {
        COUNTED.with(|counted| std::mem::take(&mut *counted.borrow_mut()))
    }

    /// How many times the fakes of this test's thread have run `load` since
    /// the last call.
    pub(crate) fn take_loads() -> usize { LOADS.with(|loads| loads.replace(0)) }

    /// `stream` counts [`Fake::STREAM_ROWS`], `materialize` counts
    /// [`Fake::MATERIALIZE_ROWS`] and, when `S` (DuckDB-shaped), returns an
    /// empty sorted batch beside the count, as DuckDB does.
    pub(crate) struct Fake<const S: bool, const P: bool, const K: bool>;

    impl<const S: bool, const P: bool, const K: bool> Fake<S, P, K> {
        pub(crate) const MATERIALIZE_ROWS: u64 = 3;
        pub(crate) const SPACE_BYTES: usize = 100;
        pub(crate) const STREAM_ROWS: u64 = 5;
    }

    impl<const S: bool, const P: bool, const K: bool> Engine for Fake<S, P, K> {
        type Built<'l>
            = u64
        where
            Self: 'l;
        type Loaded = ();
        type Settings = ();

        const HAS_SPACE: bool = P;
        const KEEPS_DUPLICATES: bool = K;
        const MATERIALIZE_MUTATES: bool = !S;
        const NAME: &'static str = "fake";
        const STREAMS: bool = S;

        fn label(_: &()) -> String { "fake".to_string() }

        fn threads(_: &()) -> Option<usize> { None }

        fn version(_: &()) -> String { "fake 1.0".to_string() }

        /// Refuses a query that reads a relation named `banned`.
        fn supports(prepared: &Prepared) -> Result<(), Unsupported> {
            if prepared.relations().contains(&"banned") {
                return Err(Unsupported {
                    reason: "relation banned".to_string(),
                });
            }
            Ok(())
        }

        fn load(_: &(), _: &Catalog, _: &Prepared) -> Result<(), EngineError> {
            LOADS.with(|loads| loads.set(loads.get() + 1));
            Ok(())
        }

        fn build<'l>(_: &'l (), _: &Catalog, _: &Prepared) -> Result<u64, EngineError>
        where
            Self: 'l,
        {
            Ok(Self::STREAM_ROWS)
        }

        fn stream(built: &u64) -> Result<u64, EngineError> {
            counted("stream");
            Ok(*built)
        }

        fn materialize(_: &u64) -> Result<(u64, Option<Tuples>), EngineError> {
            counted("materialize");
            Ok((Self::MATERIALIZE_ROWS, S.then(|| Tuples::new(0))))
        }

        fn space(_: &u64) -> Option<usize> { P.then_some(Self::SPACE_BYTES) }

        fn rows(_: &u64) -> Result<Vec<Vec<usize>>, EngineError> { Ok(Vec::new()) }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{
            test_support::{take_counted, Fake},
            *,
        },
        kermit::db::prepare,
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, Tuples},
    };

    /// DuckDB-shaped: streams, no space, bag semantics.
    type Bag = Fake<true, false, true>;
    /// Streams, no space, set semantics.
    type Set = Fake<true, false, false>;
    /// Nemo-shaped: stores its result, so it neither streams nor keeps
    /// duplicates, and has space.
    type Storing = Fake<false, true, false>;

    fn catalog(relations: &[(&str, usize, &[&[usize]])]) -> Catalog {
        let relations = relations
            .iter()
            .map(|&(name, arity, rows)| {
                let mut tuples = Tuples::new(arity);
                for row in rows {
                    tuples.push(row);
                }
                (RelationHeader::new_positional(name, arity), tuples)
            })
            .collect();
        Catalog::new(relations, StatisticsLevel::TupleCounts)
    }

    fn planner() -> Planner { Planner::new(Box::new(LexicographicOptimiser)) }

    fn prepared(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        prepare(catalog, &query, &planner(), TrieLevels::All).expect("a valid query")
    }

    /// The parser cannot write an empty head, so the query is built by clearing
    /// one: its atoms are all constants, hence all checks.
    fn checks_only(catalog: &Catalog, text: &str) -> Prepared {
        let mut query: JoinQuery = text.parse().expect("a well-formed query");
        query.head.terms.clear();
        prepare(catalog, &query, &planner(), TrieLevels::All).expect("a query of checks")
    }

    fn cell<E: Engine>() -> EngineCell<E> {
        EngineCell {
            settings: E::Settings::default(),
        }
    }

    /// `r` has two rows, `s` holds the row `(7, 8)` twice.
    fn duplicated_checks() -> Catalog {
        catalog(&[
            ("r", 1, &[&[1], &[2]]),
            ("s", 2, &[&[7, 8], &[7, 8]]),
            ("banned", 1, &[&[1]]),
        ])
    }

    #[test]
    fn an_all_constant_atoms_count_multiplies_a_bag_engines_stream() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X), s(c7, c8).");
        let cell = cell::<Bag>();
        let subject = cell.subject(&catalog, &prepared, true).unwrap();
        let built = subject.build();
        assert_eq!(subject.exercise(&built), Bag::STREAM_ROWS * 2);
        // `materialize` counts distinct rows, so the multiplicity does not
        // reach it.
        let materialized = subject.materialize(&built).unwrap();
        assert_eq!(materialized.rows, Bag::MATERIALIZE_ROWS);
        // The engine's sorted batch comes back as `kept`, for Criterion to free
        // after the timer.
        assert!(materialized.kept.is_some());
    }

    #[test]
    fn a_set_engine_clamps_the_multiplicity_to_one() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X), s(c7, c8).");
        let cell = cell::<Set>();
        let subject = cell.subject(&catalog, &prepared, true).unwrap();
        let built = subject.build();
        assert_eq!(subject.exercise(&built), Set::STREAM_ROWS);
    }

    #[test]
    fn a_check_that_counts_no_row_means_no_answers() {
        let catalog = catalog(&[("r", 1, &[&[1], &[2]]), ("s", 2, &[&[9, 9]])]);
        let prepared = prepared(&catalog, "Q(X) :- r(X), s(c7, c8).");
        let cell = cell::<Bag>();
        let subject = cell.subject(&catalog, &prepared, true).unwrap();
        let built = subject.build();
        assert_eq!(subject.exercise(&built), 0);
        assert_eq!(subject.materialize(&built).unwrap().rows, 0);
    }

    #[test]
    fn a_query_of_checks_never_calls_the_engine() {
        let catalog = duplicated_checks();
        let prepared = checks_only(&catalog, "Q(X) :- s(c7, c8).");
        assert!(prepared.indexes().is_empty());
        let cell = cell::<Bag>();
        // `Fake::supports` refuses any query that reads `banned`, checks
        // included, so this preflight passes only because a query of checks
        // never reaches the engine.
        let banned = checks_only(&catalog, "Q(X) :- banned(c1).");
        assert_eq!(banned.relations(), vec!["banned"]);
        cell.subject(&catalog, &banned, true).unwrap();
        let subject = cell.subject(&catalog, &prepared, true).unwrap();
        let built = subject.build();
        // The empty row, once per duplicate of the check's row.
        assert_eq!(subject.exercise(&built), 2);
        assert_eq!(subject.materialize(&built).unwrap().rows, 1);
        assert!(subject.heap_bytes(&built).is_empty());
        assert!(cell.subject_axes(&subject).is_empty());
    }

    #[test]
    fn the_subject_reports_the_engines_version_and_space() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X).");

        let storing = cell::<Storing>();
        let subject = storing.subject(&catalog, &prepared, true).unwrap();
        let built = subject.build();
        assert_eq!(subject.heap_bytes(&built), vec![Storing::SPACE_BYTES]);
        let axes = storing.subject_axes(&subject);
        assert_eq!(axes["engine_version"], serde_json::json!("fake 1.0"));
        assert_eq!(axes.len(), 1);
        // Nemo-shaped: the count alone, with no batch to keep.
        assert!(subject.materialize(&built).unwrap().kept.is_none());

        let bag = cell::<Bag>();
        let subject = bag.subject(&catalog, &prepared, true).unwrap();
        let built = subject.build();
        assert!(subject.heap_bytes(&built).is_empty());
    }

    #[test]
    #[should_panic(expected = "fake does not stream")]
    fn exercising_a_storing_engine_is_a_broken_invariant() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X).");
        let cell = cell::<Storing>();
        let subject = cell.subject(&catalog, &prepared, true).unwrap();
        let built = subject.build();
        let _ = subject.exercise(&built);
    }

    #[test]
    fn the_subjects_flags_follow_the_engines() {
        type StoringSubject<'a> = EngineSubject<'a, Storing>;
        type BagSubject<'a> = EngineSubject<'a, Bag>;
        const { assert!(!<StoringSubject<'_> as Subject>::STREAMS) };
        const { assert!(<StoringSubject<'_> as Subject>::MATERIALIZE_MUTATES) };
        const { assert!(<BagSubject<'_> as Subject>::STREAMS) };
        const { assert!(!<BagSubject<'_> as Subject>::MATERIALIZE_MUTATES) };
        const { assert!(!<BagSubject<'_> as Subject>::EXERCISE_MUTATES) };
    }

    #[test]
    fn preflight_refuses_a_query_the_engine_does_not_support() {
        let catalog = duplicated_checks();
        let supported = prepared(&catalog, "Q(X) :- r(X).");
        let banned = prepared(&catalog, "Q(X) :- banned(X).");
        let cell = cell::<Bag>();
        cell.subject(&catalog, &supported, true).unwrap();
        let Err(error) = cell.subject(&catalog, &banned, true) else {
            panic!("the fake refuses a query that reads `banned`");
        };
        let error = error.to_string();
        assert!(error.contains("fake cannot run query"), "{error}");
        // `Prepared::query` is the executed query, which names the index `#0`
        // after `banned`.
        assert!(error.contains("#0(X)"), "{error}");
        assert!(error.contains("relation banned"), "{error}");
    }

    #[test]
    fn preflight_runs_the_one_count_only_when_asked() {
        let catalog = duplicated_checks();
        let prepared = prepared(&catalog, "Q(X) :- r(X).");

        // A non-streaming engine counts through `materialize`: once when asked,
        // and not at all when the memory guard skips `materialize` (`count`
        // false), which still loads and builds.
        let storing = cell::<Storing>();
        let _ = take_counted();
        storing.subject(&catalog, &prepared, false).unwrap();
        assert!(take_counted().is_empty());
        storing.subject(&catalog, &prepared, true).unwrap();
        assert_eq!(take_counted(), vec!["materialize"]);

        // A streaming engine counts through `stream`; `run_benchmark` always
        // passes true for it.
        let bag = cell::<Bag>();
        bag.subject(&catalog, &prepared, true).unwrap();
        assert_eq!(take_counted(), vec!["stream"]);
    }

    #[test]
    fn preflight_without_the_count_still_checks_support() {
        let catalog = duplicated_checks();
        let banned = prepared(&catalog, "Q(X) :- banned(X).");
        let Err(error) = cell::<Storing>().subject(&catalog, &banned, false) else {
            panic!("the fake refuses a query that reads `banned`");
        };
        let error = error.to_string();
        assert!(error.contains("fake cannot run query"), "{error}");
        assert!(take_counted().is_empty());
    }

    #[test]
    fn a_cells_names_axes_and_levels() {
        let cell = cell::<Bag>();
        assert_eq!(cell.label(), "fake");
        assert_eq!(cell.group_segment(), "fake");
        assert_eq!(cell.trie_levels(), TrieLevels::All);
        let axes = cell.axes();
        assert_eq!(axes["system"], serde_json::json!("fake"));
        // The fake has no thread count, and `engine_version` comes from the
        // subject.
        assert_eq!(axes.len(), 1);
        const { assert!(!<EngineCell<Bag> as Cell>::PER_INDEX_BYTES) };
    }

    #[test]
    fn a_cell_supports_the_metrics_its_engine_can_measure() {
        let storing = cell::<Storing>();
        assert!(storing.supports(Metric::Build));
        assert!(storing.supports(Metric::Materialize));
        assert!(storing.supports(Metric::Space));
        assert!(!storing.supports(Metric::Iteration));
        assert!(!storing.supports(Metric::EndToEnd));

        let bag = cell::<Bag>();
        assert!(bag.supports(Metric::Build));
        assert!(bag.supports(Metric::Materialize));
        assert!(bag.supports(Metric::Iteration));
        assert!(bag.supports(Metric::EndToEnd));
        assert!(!bag.supports(Metric::Space));
    }

    #[test]
    fn the_skip_notes_name_each_metric_asked_for_and_not_measurable() {
        use Metric::{Build, EndToEnd, Iteration, Space};
        let storing = cell::<Storing>();
        assert_eq!(
            skip_notes(&storing, &[Build, Iteration, Space, EndToEnd], false),
            vec!["note: fake does not stream, so it skips iteration and end-to-end".to_string()]
        );
        // A probe asks for a single `iteration` run without naming it.
        let note = "note: fake does not stream, so it skips iteration and end-to-end";
        assert_eq!(skip_notes(&storing, &[Space], true), vec![note.to_string()]);
        assert!(skip_notes(&storing, &[Build, Space], false).is_empty());

        let bag = cell::<Bag>();
        assert_eq!(
            skip_notes(&bag, &[Build, Iteration, Space, EndToEnd], false),
            vec!["note: fake reports no space".to_string()]
        );
        assert!(skip_notes(&bag, &[Build, Iteration], false).is_empty());
    }
}
