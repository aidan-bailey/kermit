//! External engines: other systems' joins, run on the queries kermit prepares.
//!
//! An [`Engine`] answers the query that [`prepare`](crate::db::prepare)
//! decided. It reads the executed query (one atom `#k` per index, over the
//! atom's variables in index-column order), each index's
//! [`IndexSpec`](kermit_algos::IndexSpec) and the plan, and it never parses
//! Datalog. Constants and repeated variables are already filters on the specs,
//! so an engine sees plain equi-joins. It answers the *full* conjunctive query:
//! every variable of the executed query, in the executed query's canonical
//! order (`canonical_names(prepared.query())`), so that on duplicate-free
//! inputs its count equals kermit's bag count and the benchmark's `expected`.
//!
//! An all-constant atom is not an atom of the executed query. It is a check,
//! counted once by the lifecycle's `check_multiplicity`, so an engine never
//! sees one.
//!
//! `sql` is the translator that SQL engines share. Each engine lives in its own
//! module, behind its own cargo feature (off by default), so that a default
//! build compiles none of them.

#[cfg(feature = "engine-duckdb")]
pub mod duckdb;
#[cfg(feature = "engine-nemo")]
pub mod nemo;
pub mod sql;

use {
    crate::db::{Catalog, Prepared},
    kermit_ds::Tuples,
    std::{error::Error, fmt},
};

/// One external system, in kermit's phases.
///
/// `load` is untimed and puts the query's relations inside the engine. `build`
/// is timed and leaves what the join needs. `stream` and `materialize` are the
/// two timed joins. A single `Settings` value (a thread count, say) selects the
/// engine's configuration.
///
/// Every method is fallible, but a timed body cannot return an error. So the
/// binary runs one fallible preflight per query (`load`, `build`, then one
/// count), untimed, and the timed bodies then `expect`: an error that first
/// appears after a clean preflight is a broken invariant.
pub trait Engine {
    /// The report's `system` axis: `"duckdb"`, `"nemo"`.
    const NAME: &'static str;
    /// Whether [`stream`](Engine::stream) is implemented, which `iteration` and
    /// `end_to_end` need.
    const STREAMS: bool;
    /// Whether an all-constant atom's row count multiplies the results (bag) or
    /// is clamped to 1 (set), as `Relation::KEEPS_DUPLICATES` says for kermit's
    /// structures. DuckDB: true. Nemo: false.
    const KEEPS_DUPLICATES: bool;
    /// Whether [`materialize`](Engine::materialize) changes the built state
    /// (Nemo stores its result), so that every timed sample needs a fresh
    /// build.
    const MATERIALIZE_MUTATES: bool;
    /// Whether [`space`](Engine::space) reports anything. DuckDB keeps no index
    /// and reports none.
    const HAS_SPACE: bool;

    /// The engine's configuration. DuckDB: its thread count (default 1). Nemo:
    /// `()`.
    type Settings: Default + Clone + fmt::Debug;
    /// The query's relations inside the engine: what `load` leaves, untimed.
    type Loaded;
    /// What one timed `build` leaves. It may borrow the
    /// [`Loaded`](Engine::Loaded) state it was built from.
    type Built<'l>
    where
        Self: 'l;

    /// The Criterion group segment and stderr label: `"duckdb-t1"`, `"nemo"`.
    fn label(settings: &Self::Settings) -> String;
    /// The `engine_threads` axis: `Some(n)` for DuckDB, `None` for Nemo.
    fn threads(settings: &Self::Settings) -> Option<usize>;
    /// The `engine_version` axis, read from the running engine (DuckDB:
    /// `pragma_version()`).
    fn version(loaded: &Self::Loaded) -> String;
    /// Whether the engine can run `prepared`, checked before anything is
    /// loaded.
    ///
    /// # Errors
    ///
    /// The [`Unsupported`] reason, when it cannot.
    fn supports(prepared: &Prepared) -> Result<(), Unsupported>;
    /// Puts the relations `prepared` reads inside the engine. Untimed.
    ///
    /// # Errors
    ///
    /// The engine's failure to set up or to load a relation.
    fn load(
        settings: &Self::Settings, catalog: &Catalog, prepared: &Prepared,
    ) -> Result<Self::Loaded, EngineError>;
    /// The timed build: one filtered copy per `IndexSpec`, and whatever the
    /// engine builds from it. It takes the catalog because Nemo's providers
    /// stream from `Catalog::shared_tuples`.
    ///
    /// # Errors
    ///
    /// The engine's failure to build.
    fn build<'l>(
        loaded: &'l Self::Loaded, catalog: &Catalog, prepared: &Prepared,
    ) -> Result<Self::Built<'l>, EngineError>
    where
        Self: 'l;
    /// The streamed, counted join: the `iteration` metric. Only called when
    /// [`STREAMS`](Engine::STREAMS) is true.
    ///
    /// # Errors
    ///
    /// The engine's failure to run the join.
    fn stream(built: &Self::Built<'_>) -> Result<u64, EngineError>;
    /// The join with its result stored, then the count of distinct full
    /// assignments: the `materialize` metric. The count comes with the sorted
    /// batch when the engine copies its rows into `Tuples` (DuckDB: `Some`,
    /// from `materialize_count`; Nemo: `None`), returned so that
    /// Criterion frees it after the timer stops. `EngineSubject` maps it onto
    /// `Materialized { rows, kept }`.
    ///
    /// # Errors
    ///
    /// The engine's failure to run the join.
    fn materialize(built: &Self::Built<'_>) -> Result<(u64, Option<Tuples>), EngineError>;
    /// The exact space of what `build` left, in bytes. `Some` only if
    /// [`HAS_SPACE`](Engine::HAS_SPACE).
    fn space(built: &Self::Built<'_>) -> Option<usize>;
    /// The full assignments, in the executed query's canonical order, for
    /// tests; never timed. Nemo runs its plan first if `materialize` has not.
    ///
    /// # Errors
    ///
    /// The engine's failure to run the join.
    fn rows(built: &Self::Built<'_>) -> Result<Vec<Vec<usize>>, EngineError>;
}

/// Why an engine cannot run a prepared query, as [`Engine::supports`] reports
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    /// What the engine cannot do with the query.
    pub reason: String,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported: {}", self.reason)
    }
}

impl Error for Unsupported {}

/// An engine's failure to load, build or run a query.
#[derive(Debug)]
pub struct EngineError {
    /// The failing engine's [`Engine::NAME`].
    pub engine: &'static str,
    /// What went wrong, in the engine's own words.
    pub message: String,
}

impl EngineError {
    /// An error of `engine` (its [`Engine::NAME`]), in the engine's own words.
    pub fn new(engine: &'static str, message: impl fmt::Display) -> Self {
        EngineError {
            engine,
            message: message.to_string(),
        }
    }
}

/// The relations `prepared`'s indexes read, each once, in index order: what an
/// engine's `load` puts inside it. A relation that only an all-constant atom
/// reads is not among them, since the lifecycle's `check_multiplicity` counts
/// it from the catalog.
pub fn indexed_relations(prepared: &Prepared) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for spec in prepared.indexes() {
        if !names.contains(&spec.relation.as_str()) {
            names.push(&spec.relation);
        }
    }
    names
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.engine, self.message)
    }
}

impl Error for EngineError {}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::prepare,
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, TrieLevels, Tuples},
    };

    /// Two binary relations, `r` and `s`, of one row each.
    fn catalog() -> Catalog {
        let relation = |name: &str| {
            let mut tuples = Tuples::new(2);
            tuples.push(&[1, 2]);
            (RelationHeader::new_positional(name, 2), tuples)
        };
        Catalog::new(
            vec![relation("r"), relation("s")],
            StatisticsLevel::TupleCounts,
        )
    }

    fn prepared(catalog: &Catalog) -> Prepared {
        let query: JoinQuery = "Q(X, Y, Z) :- r(X, Y), s(Y, Z)."
            .parse()
            .expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    /// An engine whose built state borrows what it loaded, as DuckDB's does,
    /// and which cannot materialize.
    struct RelationNames;

    impl Engine for RelationNames {
        type Built<'l> = &'l [String];
        type Loaded = Vec<String>;
        type Settings = ();

        const HAS_SPACE: bool = false;
        const KEEPS_DUPLICATES: bool = false;
        const MATERIALIZE_MUTATES: bool = false;
        const NAME: &'static str = "relation-names";
        const STREAMS: bool = true;

        fn label(_settings: &()) -> String { "relation-names".to_string() }

        fn threads(_settings: &()) -> Option<usize> { None }

        fn version(loaded: &Vec<String>) -> String { format!("{} relations", loaded.len()) }

        fn supports(_prepared: &Prepared) -> Result<(), Unsupported> { Ok(()) }

        fn load(
            _settings: &(), _catalog: &Catalog, prepared: &Prepared,
        ) -> Result<Vec<String>, EngineError> {
            Ok(prepared
                .relations()
                .into_iter()
                .map(str::to_string)
                .collect())
        }

        fn build<'l>(
            loaded: &'l Vec<String>, _catalog: &Catalog, _prepared: &Prepared,
        ) -> Result<&'l [String], EngineError>
        where
            Self: 'l,
        {
            Ok(loaded)
        }

        fn stream(built: &&[String]) -> Result<u64, EngineError> { Ok(built.len() as u64) }

        fn materialize(_built: &&[String]) -> Result<(u64, Option<Tuples>), EngineError> {
            Err(EngineError {
                engine: Self::NAME,
                message: "cannot materialize".to_string(),
            })
        }

        fn space(_built: &&[String]) -> Option<usize> { None }

        fn rows(built: &&[String]) -> Result<Vec<Vec<usize>>, EngineError> {
            Ok(vec![vec![built.len()]])
        }
    }

    #[test]
    fn a_built_state_may_borrow_what_the_engine_loaded() {
        let catalog = catalog();
        let prepared = prepared(&catalog);
        let loaded = RelationNames::load(&(), &catalog, &prepared).expect("loads");
        let built = RelationNames::build(&loaded, &catalog, &prepared).expect("builds");
        assert_eq!(RelationNames::version(&loaded), "2 relations");
        assert_eq!(RelationNames::stream(&built).expect("streams"), 2);
        assert_eq!(RelationNames::rows(&built).expect("has rows"), vec![vec![
            2
        ]]);
        assert_eq!(RelationNames::label(&()), "relation-names");
        assert_eq!(RelationNames::threads(&()), None);
        assert_eq!(RelationNames::space(&built), None);
        assert!(RelationNames::supports(&prepared).is_ok());
    }

    #[test]
    fn an_engine_error_names_its_engine() {
        let catalog = catalog();
        let prepared = prepared(&catalog);
        let loaded = RelationNames::load(&(), &catalog, &prepared).expect("loads");
        let built = RelationNames::build(&loaded, &catalog, &prepared).expect("builds");
        let error = RelationNames::materialize(&built).expect_err("this engine cannot materialize");
        assert_eq!(error.engine, "relation-names");
        assert_eq!(error.to_string(), "relation-names: cannot materialize");
        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert!(boxed.source().is_none());
    }

    #[test]
    fn an_engine_error_is_made_from_any_message() {
        let error = EngineError::new("duckdb", format_args!("step {}", 3));
        assert_eq!(error.engine, "duckdb");
        assert_eq!(error.to_string(), "duckdb: step 3");
    }

    /// Two indexes read `r` (one filtered by its constant), one reads `s`, and
    /// `t` is read only by an all-constant atom: a check, which the lifecycle
    /// counts from the catalog, so no engine loads it.
    #[test]
    fn the_indexed_relations_are_each_listed_once_without_the_checks() {
        let relation = |name: &str| {
            let mut tuples = Tuples::new(2);
            tuples.push(&[1, 2]);
            (RelationHeader::new_positional(name, 2), tuples)
        };
        let catalog = Catalog::new(
            vec![relation("r"), relation("s"), relation("t")],
            StatisticsLevel::TupleCounts,
        );
        let query: JoinQuery = "Q(X, Y) :- r(X, Y), s(Y, X), r(Y, c1), t(c2, c3)."
            .parse()
            .expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        let prepared = prepare(&catalog, &query, &planner, TrieLevels::All).expect("a valid query");
        assert_eq!(prepared.indexes().len(), 3);
        assert_eq!(prepared.relations(), vec!["r", "s", "t"]);
        assert_eq!(indexed_relations(&prepared), vec!["r", "s"]);
    }

    #[test]
    fn an_unsupported_query_gives_its_reason() {
        let unsupported = Unsupported {
            reason: "a ten-way join".to_string(),
        };
        assert_eq!(unsupported.to_string(), "unsupported: a ten-way join");
        assert_eq!(unsupported.clone(), unsupported);
    }
}
