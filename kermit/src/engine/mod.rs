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
//! build compiles none of them. [`EngineKind`] names every engine whether or
//! not it is compiled in, and its [`Capabilities`] are the one statement of
//! what the runner may ask of it.

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

/// The external engines kermit can run, as `--engine` names them, whether or
/// not this build compiled them in.
///
/// Each variant's [`Capabilities`] are the one statement of what the runner
/// may ask of that engine: its [`Engine`] impl takes them as
/// [`Engine::CAPABILITIES`], and the binary reads them before any engine runs,
/// to name a sweep's Criterion groups and to refuse an engine this build lacks.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum EngineKind {
    /// DuckDB, in-process (feature `engine-duckdb`).
    Duckdb,
    /// Nemo, in-process (feature `engine-nemo`).
    Nemo,
}

impl EngineKind {
    /// What the runner may ask of this engine.
    pub const fn capabilities(self) -> Capabilities {
        match self {
            | Self::Duckdb => Capabilities {
                name: "duckdb",
                threaded: true,
                streams: true,
                keeps_duplicates: true,
                materialize_mutates: false,
                has_space: false,
            },
            | Self::Nemo => Capabilities {
                name: "nemo",
                threaded: false,
                streams: false,
                keeps_duplicates: false,
                materialize_mutates: true,
                has_space: true,
            },
        }
    }

    /// Whether this build compiled the engine in: its cargo feature,
    /// `engine-{name}`, is on.
    pub const fn compiled(self) -> bool {
        match self {
            | Self::Duckdb => cfg!(feature = "engine-duckdb"),
            | Self::Nemo => cfg!(feature = "engine-nemo"),
        }
    }
}

/// What the runner may ask of an engine, known before it runs. It is plain
/// data, so that a build without the engine's feature can still name and plan
/// the engine's cells.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// The report's `system` axis, the `--engine` value and the suffix of the
    /// cargo feature (`engine-{name}`): `"duckdb"`, `"nemo"`.
    pub name: &'static str,
    /// Whether the engine takes a thread count (`--engine-threads`), which
    /// then names its cell (`duckdb-t1`) and fills the `engine_threads` axis.
    /// DuckDB: true. Nemo, which is single-threaded: false.
    pub threaded: bool,
    /// Whether [`Engine::stream`] is implemented, which `iteration` and
    /// `end_to_end` need. DuckDB: true. Nemo: false.
    pub streams: bool,
    /// Whether an all-constant atom's row count multiplies the results (bag)
    /// or is clamped to 1 (set), as `Relation::KEEPS_DUPLICATES` says for
    /// kermit's structures. DuckDB: true. Nemo: false.
    pub keeps_duplicates: bool,
    /// Whether [`Engine::materialize`] changes the built state (Nemo stores
    /// its result), so that every timed sample needs a fresh build.
    pub materialize_mutates: bool,
    /// Whether [`Engine::space`] reports anything. DuckDB keeps no index and
    /// reports none.
    pub has_space: bool,
}

impl Capabilities {
    /// The Criterion group segment and stderr label of the engine run on
    /// `threads` threads: `duckdb-t4`. An engine that takes no thread count is
    /// its name alone (`nemo`), whatever `threads` holds.
    pub fn label(&self, threads: usize) -> String {
        if self.threaded {
            format!("{}-t{threads}", self.name)
        } else {
            self.name.to_string()
        }
    }
}

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
    /// What the runner may ask of this engine. A real engine's are its
    /// [`EngineKind`]'s (`EngineKind::Duckdb.capabilities()`), so that the
    /// binary, which reads them without the engine's type, cannot disagree
    /// with the engine.
    const CAPABILITIES: Capabilities;

    /// The engine's configuration. DuckDB: its thread count (default 1) and
    /// where it spills. Nemo: `()`.
    type Settings: Default + Clone + fmt::Debug;
    /// The query's relations inside the engine: what `load` leaves, untimed.
    type Loaded;
    /// What one timed `build` leaves. It may borrow the
    /// [`Loaded`](Engine::Loaded) state it was built from.
    type Built<'l>
    where
        Self: 'l;

    /// The thread count `settings` runs the engine on: DuckDB's `PRAGMA
    /// threads`, and 1 for an engine that takes none
    /// ([`Capabilities::threaded`] false).
    fn threads(settings: &Self::Settings) -> usize;
    /// The `engine_version` axis. DuckDB reads it from the running library
    /// (`pragma_version()`); Nemo, compiled in at a pinned revision, names that
    /// revision.
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
    /// engine builds from it. It reads the rows from what `load` left (DuckDB
    /// its base tables, Nemo its handles on the catalog's tuples).
    ///
    /// # Errors
    ///
    /// The engine's failure to build.
    fn build<'l>(
        loaded: &'l Self::Loaded, prepared: &Prepared,
    ) -> Result<Self::Built<'l>, EngineError>
    where
        Self: 'l;
    /// The streamed, counted join: the `iteration` metric. Only called when
    /// [`Capabilities::streams`] is true.
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
    /// [`Capabilities::has_space`].
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
    /// The failing engine's name ([`Capabilities::name`]).
    pub engine: &'static str,
    /// What went wrong, in the engine's own words.
    pub message: String,
}

impl EngineError {
    /// An error of `engine` (its [`Capabilities::name`]), in the engine's own
    /// words.
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

        const CAPABILITIES: Capabilities = Capabilities {
            name: "relation-names",
            threaded: false,
            streams: true,
            keeps_duplicates: false,
            materialize_mutates: false,
            has_space: false,
        };

        fn threads(_settings: &()) -> usize { 1 }

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
            loaded: &'l Vec<String>, _prepared: &Prepared,
        ) -> Result<&'l [String], EngineError>
        where
            Self: 'l,
        {
            Ok(loaded)
        }

        fn stream(built: &&[String]) -> Result<u64, EngineError> { Ok(built.len() as u64) }

        fn materialize(_built: &&[String]) -> Result<(u64, Option<Tuples>), EngineError> {
            Err(EngineError::new(
                Self::CAPABILITIES.name,
                "cannot materialize",
            ))
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
        let built = RelationNames::build(&loaded, &prepared).expect("builds");
        assert_eq!(RelationNames::version(&loaded), "2 relations");
        assert_eq!(RelationNames::stream(&built).expect("streams"), 2);
        assert_eq!(RelationNames::rows(&built).expect("has rows"), vec![vec![
            2
        ]]);
        assert_eq!(RelationNames::threads(&()), 1);
        assert_eq!(RelationNames::space(&built), None);
        assert!(RelationNames::supports(&prepared).is_ok());
    }

    #[test]
    fn an_engine_error_names_its_engine() {
        let catalog = catalog();
        let prepared = prepared(&catalog);
        let loaded = RelationNames::load(&(), &catalog, &prepared).expect("loads");
        let built = RelationNames::build(&loaded, &prepared).expect("builds");
        let error = RelationNames::materialize(&built).expect_err("this engine cannot materialize");
        assert_eq!(error.engine, "relation-names");
        assert_eq!(error.to_string(), "relation-names: cannot materialize");
        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert!(boxed.source().is_none());
    }

    /// The `--engine` value is the capabilities' name, which is also the cargo
    /// feature's suffix and the report's `system` axis.
    #[test]
    fn every_engines_flag_value_is_its_name() {
        use clap::ValueEnum;
        for kind in EngineKind::value_variants() {
            let value = kind.to_possible_value().expect("no engine is hidden");
            assert_eq!(value.get_name(), kind.capabilities().name);
        }
    }

    #[test]
    fn a_threaded_engines_label_carries_its_thread_count() {
        assert_eq!(EngineKind::Duckdb.capabilities().label(4), "duckdb-t4");
        assert_eq!(
            EngineKind::Nemo.capabilities().label(4),
            "nemo",
            "an engine that takes no thread count is named alone"
        );
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
