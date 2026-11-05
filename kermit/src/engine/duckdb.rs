//! DuckDB, run in-process through the `duckdb` crate (feature `engine-duckdb`).
//!
//! DuckDB answers the executed query as SQL (see [`sql`](super::sql)) over one
//! temporary table per index spec, and sits in kermit's phases like this:
//! - `load` (untimed): an in-memory database with `PRAGMA threads`, a memory
//!   limit and a private spill directory on disk ([`default_spill_parent`]).
//!   Each relation an index reads is appended from the catalog as `UBIGINT`
//!   columns.
//! - `build` (timed): one `CREATE TEMP TABLE` per spec, holding its filtered,
//!   projected rows (one copy, and no index), then the join statement prepared.
//! - `stream`: the prepared statement streamed as Arrow batches, whose rows are
//!   counted. DuckDB builds its hash tables here.
//! - `materialize`: the same statement, every batch copied into kermit
//!   `Tuples`, then [`materialize_count`]; the count and the sorted batch are
//!   both returned, so the batch is freed after the timer.
//! - `space`: not reported, since DuckDB keeps no index.
//!
//! DuckDB's index work sits in `stream` while kermit's sits in `build`, so the
//! two compare on `end_to_end`. The statement is prepared at the end of `build`
//! (decision 11 of the design), so `iteration` never includes DuckDB's
//! planning, and `build` and each `end_to_end` sample include it once.
//!
//! A relation whose name differs from another's only in case would share a
//! table with it, since DuckDB compares quoted identifiers without regard to
//! case: the load then fails with the database's "already exists" error.

use {
    super::{
        indexed_relations,
        sql::{
            column, create_spec_table, join_select, relation_table, relation_table_name, spec_table,
        },
        Capabilities, Engine, EngineError, EngineKind, Unsupported,
    },
    crate::db::{materialize_count, Catalog, Prepared},
    ::duckdb::{
        appender_params_from_iter,
        arrow::{
            array::{Array, UInt64Array},
            datatypes::{DataType, Field, Schema, SchemaRef},
        },
        Connection, Statement,
    },
    kermit_ds::Tuples,
    std::{
        cell::{Cell, RefCell},
        fmt::Display,
        fs::{self, File},
        io,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    },
};

/// DuckDB's `memory_limit` on every connection. It sits under the sweeps' 14
/// GiB cap, where DuckDB's own default is 80% of the host's RAM.
const MEMORY_LIMIT: &str = "8GB";

/// The DuckDB engine.
pub struct DuckDb;

/// The settings of a [`DuckDb`] engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuckDbSettings {
    /// DuckDB's `PRAGMA threads`. One thread gives the parallelism of kermit's
    /// joins and serial builds.
    pub threads: usize,
    /// The directory each connection makes its private spill directory in.
    /// Default: [`default_spill_parent`], on disk.
    pub spill_parent: PathBuf,
}

impl Default for DuckDbSettings {
    fn default() -> Self {
        DuckDbSettings {
            threads: 1,
            spill_parent: default_spill_parent(),
        }
    }
}

/// The default [`DuckDbSettings::spill_parent`]: `<platform cache>/kermit/
/// duckdb.tmp`, beside the benchmark cache, as the JOB generator's raw cache
/// keeps its own `duckdb.tmp`.
///
/// Not the system's temporary directory: where `/tmp` is a tmpfs, as on the
/// host the sweeps run on, a spill there is memory, charged to the same limit
/// as the run it was meant to relieve, and a killed run's spill stays resident
/// until it is deleted by hand. The temporary directory is the fallback only
/// on a platform with no cache directory.
pub fn default_spill_parent() -> PathBuf {
    kermit_bench::cache::base_cache_dir()
        .ok()
        .and_then(|benchmarks| benchmarks.parent().map(|kermit| kermit.join("duckdb.tmp")))
        .unwrap_or_else(std::env::temp_dir)
}

/// The start of every spill directory's name: `kermit-duckdb-<process
/// id>-<sequence>`.
const SPILL_PREFIX: &str = "kermit-duckdb-";

/// The lock file in each spill directory, locked for as long as its guard
/// lives.
const LOCK_FILE: &str = "kermit.lock";

/// The name the lock file is locked under before it is renamed to
/// [`LOCK_FILE`], so that no one finds a live directory's lock file unlocked.
const STAGED_LOCK_FILE: &str = "kermit.lock.new";

/// One connection's private spill directory, `kermit-duckdb-<process
/// id>-<sequence>` under [`DuckDbSettings::spill_parent`], removed when the
/// guard drops.
///
/// DuckDB deletes every spill file in its `temp_directory` when a connection
/// closes, so two connections that shared a directory would corrupt each
/// other's spills (a run could finish with wrong rows). The process id keeps
/// two processes apart and the sequence keeps two connections of this process
/// apart, as the JOB generator's `SpillDir` does for its calls.
///
/// A process that is killed outright (earlyoom's SIGKILL) never drops its
/// guards, so the directory holds a [`LOCK_FILE`], locked while the guard
/// lives. The OS frees a dead process's locks, so a directory whose lock can be
/// taken was abandoned, and the next spill directory made beside it removes it
/// ([`remove_abandoned`]).
struct SpillDirectory {
    path: PathBuf,
    /// Held for its lock and never read. It closes, and unlocks, after `drop`
    /// has removed the directory.
    _lock: File,
}

impl SpillDirectory {
    /// Makes a fresh, empty directory under `parent` (and `parent`, if
    /// missing), first removing whatever spill directories there were
    /// abandoned.
    fn create(parent: &Path) -> Result<SpillDirectory, EngineError> {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        Self::create_with(parent, || SEQUENCE.fetch_add(1, Ordering::Relaxed))
    }

    /// [`create`](Self::create), numbering the directory by `next_sequence`.
    ///
    /// A name already taken was left by a process with this one's id that has
    /// since gone (no live process holds this process's id), so it is skipped
    /// for the next number. It is not deleted here: only a lock that can be
    /// taken shows that a directory was abandoned.
    fn create_with(
        parent: &Path, mut next_sequence: impl FnMut() -> usize,
    ) -> Result<SpillDirectory, EngineError> {
        fs::create_dir_all(parent).map_err(|e| {
            failed(
                &format!("creating the spill parent {}", parent.display()),
                e,
            )
        })?;
        remove_abandoned(parent);
        let path = loop {
            let name = format!("{SPILL_PREFIX}{}-{}", std::process::id(), next_sequence());
            let path = parent.join(name);
            match fs::create_dir(&path) {
                | Ok(()) => break path,
                | Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {},
                | Err(e) => {
                    return Err(failed(
                        &format!("creating the spill directory {}", path.display()),
                        e,
                    ));
                },
            }
        };
        match lock(&path) {
            | Ok(lock) => Ok(SpillDirectory {
                path,
                _lock: lock,
            }),
            | Err(e) => {
                // No guard owns the directory yet, so it is removed here.
                let _ = fs::remove_dir_all(&path);
                Err(failed(
                    &format!("locking the spill directory {}", path.display()),
                    e,
                ))
            },
        }
    }

    /// The path as the text of a SQL statement, which must be valid UTF-8.
    fn text(&self) -> Result<&str, EngineError> {
        self.path.to_str().ok_or_else(|| {
            error(format!(
                "the spill directory {} is not valid UTF-8",
                self.path.display()
            ))
        })
    }
}

impl Drop for SpillDirectory {
    fn drop(&mut self) {
        // Already gone is fine, and a directory left behind is removed by the
        // next spill directory made beside it once this lock is freed: nothing
        // more can be done from a destructor.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Locks `directory`'s [`LOCK_FILE`]: under [`STAGED_LOCK_FILE`] first, then
/// renamed into place, so that [`remove_abandoned`] never finds it unlocked.
fn lock(directory: &Path) -> io::Result<File> {
    let staged = directory.join(STAGED_LOCK_FILE);
    let lock = File::create(&staged)?;
    lock.lock()?;
    fs::rename(&staged, directory.join(LOCK_FILE))?;
    Ok(lock)
}

/// Removes each spill directory under `parent` whose lock can be taken: its
/// owner has died. A spill directory with no lock file (one being made) and a
/// directory of another name are left alone, as is one that fails to go,
/// which a later run removes.
fn remove_abandoned(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let named = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(SPILL_PREFIX));
        let Ok(lock) = File::open(entry.path().join(LOCK_FILE)) else {
            continue;
        };
        // The lock is held while the directory goes, and freed when `lock`
        // drops.
        if named && lock.try_lock().is_ok() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// What [`DuckDb`]'s `load` leaves: an in-memory database holding the relations
/// the query's indexes read.
pub struct DuckDbLoaded {
    connection: Connection,
    /// The suffix of the next build's tables, so that builds of one load can
    /// coexist.
    builds: Cell<u64>,
    version: String,
    /// Held for its `Drop` and never read. Declared last, so that the
    /// connection closes the database before the directory goes.
    _spill: SpillDirectory,
}

/// What one timed `build` leaves: the spec tables, dropped with it, and the
/// prepared join statement.
pub struct DuckDbBuilt<'l> {
    /// `stream_arrow` needs `&mut Statement` and `Engine::stream` has `&Built`,
    /// so the statement is borrowed through a `RefCell`; nothing observable
    /// changes between runs. Declared first, so that it drops before the tables
    /// it reads.
    statement: RefCell<Statement<'l>>,
    /// The schema `stream_arrow` decodes batches with: `width` `UInt64`
    /// columns.
    schema: SchemaRef,
    /// The executed query's variable count: the width of a row.
    width: usize,
    /// Held for its `Drop` and never read: it drops the spec tables after the
    /// statement.
    _tables: Tables<'l>,
}

/// The spec tables of one build, dropped with it, even when the build fails
/// half way.
struct Tables<'l> {
    connection: &'l Connection,
    /// Quoted names, pushed before each `CREATE` so that a failed one is still
    /// cleaned up.
    names: Vec<String>,
}

impl Drop for Tables<'_> {
    fn drop(&mut self) {
        if self.names.is_empty() {
            return;
        }
        let drops: Vec<String> = self
            .names
            .iter()
            .map(|name| format!("DROP TABLE IF EXISTS {name};"))
            .collect();
        // A failure leaves the tables to the connection's own drop.
        let _ = self.connection.execute_batch(&drops.join(" "));
    }
}

/// An error of this engine, its message as written.
fn error(message: impl Display) -> EngineError {
    EngineError::new(DuckDb::CAPABILITIES.name, message)
}

/// An error of this engine: the step that failed, and DuckDB's own message.
fn failed(step: &str, source: impl Display) -> EngineError { error(format!("{step}: {source}")) }

/// `text` as a single-quoted SQL string literal.
fn string_literal(text: &str) -> String { format!("'{}'", text.replace('\'', "''")) }

/// Creates `relation`'s base table, `UBIGINT` columns `c0…`, and appends the
/// catalog's rows.
fn load_relation(
    connection: &Connection, relation: &str, tuples: &Tuples,
) -> Result<(), EngineError> {
    let context = |step: &str| format!("{step} `{relation}`");
    let columns: Vec<String> = (0..tuples.arity())
        .map(|index| format!("{} UBIGINT", column(index)))
        .collect();
    connection
        .execute_batch(&format!(
            "CREATE TABLE {} ({})",
            relation_table(relation),
            columns.join(", ")
        ))
        .map_err(|e| failed(&context("creating the table of"), e))?;
    let mut appender = connection
        .appender(&relation_table_name(relation))
        .map_err(|e| failed(&context("opening an appender on"), e))?;
    for row in tuples.rows() {
        appender
            .append_row(appender_params_from_iter(row.iter()))
            .map_err(|e| failed(&context("loading"), e))?;
    }
    appender
        .flush()
        .map_err(|e| failed(&context("flushing"), e))
}

/// The schema of a join's result: `width` `UInt64` columns.
fn result_schema(width: usize) -> SchemaRef {
    let fields: Vec<Field> = (0..width)
        .map(|index| Field::new(format!("v{index}"), DataType::UInt64, true))
        .collect();
    Arc::new(Schema::new(fields))
}

/// Runs the prepared join and copies every row into a flat batch, in the
/// statement's order.
fn collect_rows(built: &DuckDbBuilt<'_>) -> Result<Tuples, EngineError> {
    let mut statement = built.statement.borrow_mut();
    let batches = statement
        .stream_arrow([], built.schema.clone())
        .map_err(|e| failed("running the join", e))?;
    let mut rows = Tuples::new(built.width);
    let mut row = vec![0usize; built.width];
    for batch in batches {
        // `stream_arrow` decodes each chunk with the schema passed to it, so
        // the batch has `width` `UInt64` columns.
        let columns: Vec<&UInt64Array> = batch
            .columns()
            .iter()
            .map(|array| {
                array
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .expect("the batch has the schema's UInt64 columns")
            })
            .collect();
        for at in 0..batch.num_rows() {
            for (slot, array) in row.iter_mut().zip(&columns) {
                *slot = array.value(at) as usize;
            }
            rows.push(&row);
        }
    }
    Ok(rows)
}

impl Engine for DuckDb {
    type Built<'l> = DuckDbBuilt<'l>;
    type Loaded = DuckDbLoaded;
    type Settings = DuckDbSettings;

    const CAPABILITIES: Capabilities = EngineKind::Duckdb.capabilities();

    fn threads(settings: &DuckDbSettings) -> usize { settings.threads }

    fn version(loaded: &DuckDbLoaded) -> String { loaded.version.clone() }

    fn supports(_prepared: &Prepared) -> Result<(), Unsupported> { Ok(()) }

    fn load(
        settings: &DuckDbSettings, catalog: &Catalog, prepared: &Prepared,
    ) -> Result<DuckDbLoaded, EngineError> {
        if settings.threads == 0 {
            return Err(error("the thread count must be at least 1".to_string()));
        }
        // Declared before the connection, so that on every return the
        // connection drops first and closes the database before its spill
        // directory is removed.
        let spill = SpillDirectory::create(&settings.spill_parent)?;
        let connection =
            Connection::open_in_memory().map_err(|e| failed("opening an in-memory database", e))?;
        connection
            .execute_batch(&format!(
                "PRAGMA threads = {}; SET memory_limit = '{MEMORY_LIMIT}'; SET temp_directory = \
                 {};",
                settings.threads,
                string_literal(spill.text()?),
            ))
            .map_err(|e| failed("configuring the connection", e))?;
        for relation in indexed_relations(prepared) {
            let tuples = catalog
                .tuples(relation)
                .ok_or_else(|| error(format!("the catalog holds no relation `{relation}`")))?;
            load_relation(&connection, relation, tuples)?;
        }
        let version = connection
            .query_row("SELECT library_version FROM pragma_version()", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| failed("reading the library version", e))?;
        Ok(DuckDbLoaded {
            connection,
            builds: Cell::new(0),
            version,
            _spill: spill,
        })
    }

    fn build<'l>(
        loaded: &'l DuckDbLoaded, prepared: &Prepared,
    ) -> Result<DuckDbBuilt<'l>, EngineError>
    where
        Self: 'l,
    {
        let build = loaded.builds.get();
        loaded.builds.set(build + 1);
        let mut tables = Tables {
            connection: &loaded.connection,
            names: Vec::new(),
        };
        for (index, spec) in prepared.indexes().iter().enumerate() {
            tables.names.push(spec_table(index, build));
            loaded
                .connection
                .execute_batch(&create_spec_table(spec, index, build))
                .map_err(|e| failed(&format!("building the table of index #{index}"), e))?;
        }
        // The last step of the build (decision 11): `stream` then never
        // includes planning.
        let statement = loaded
            .connection
            .prepare(&join_select(prepared, build))
            .map_err(|e| failed("preparing the join", e))?;
        let width = prepared.assignment_width();
        Ok(DuckDbBuilt {
            statement: RefCell::new(statement),
            schema: result_schema(width),
            width,
            _tables: tables,
        })
    }

    fn stream(built: &DuckDbBuilt<'_>) -> Result<u64, EngineError> {
        let mut statement = built.statement.borrow_mut();
        let batches = statement
            .stream_arrow([], built.schema.clone())
            .map_err(|e| failed("running the join", e))?;
        Ok(batches.map(|batch| batch.num_rows() as u64).sum())
    }

    fn materialize(built: &DuckDbBuilt<'_>) -> Result<(u64, Option<Tuples>), EngineError> {
        // The sorted batch is returned with the count, so it is freed after the
        // timer stops.
        let (sorted, distinct) = materialize_count(collect_rows(built)?);
        Ok((distinct, Some(sorted)))
    }

    fn space(_built: &DuckDbBuilt<'_>) -> Option<usize> { None }

    fn rows(built: &DuckDbBuilt<'_>) -> Result<Vec<Vec<usize>>, EngineError> {
        Ok(collect_rows(built)?.to_vecs())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::prepare,
        kermit_algos::{JoinQuery, LexicographicOptimiser, Planner, StatisticsLevel},
        kermit_ds::{RelationHeader, TrieLevels},
    };

    const TRIANGLE: &str = "Q(X, Y, Z) :- e(X, Y), e(Y, Z), e(X, Z).";

    /// A catalog of relations, each `(name, arity, rows)`.
    fn catalog(relations: Vec<(&str, usize, Vec<Vec<usize>>)>) -> Catalog {
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
        Catalog::new(relations, StatisticsLevel::TupleCounts)
    }

    /// The edge `1 -> 2` twice, then `2 -> 3` and `1 -> 3`.
    fn graph() -> Catalog {
        catalog(vec![("e", 2, vec![
            vec![1, 2],
            vec![2, 3],
            vec![1, 3],
            vec![1, 2],
        ])])
    }

    fn prepared(catalog: &Catalog, text: &str) -> Prepared {
        let query: JoinQuery = text.parse().expect("a well-formed query");
        let planner = Planner::new(Box::new(LexicographicOptimiser));
        prepare(catalog, &query, &planner, TrieLevels::All).expect("a valid query")
    }

    /// Settings on `threads` threads whose spill directories go under the
    /// system's temporary directory: these tests spill nothing, and their
    /// directories do not belong in the user's cache.
    fn settings(threads: usize) -> DuckDbSettings {
        DuckDbSettings {
            threads,
            spill_parent: std::env::temp_dir(),
        }
    }

    fn load(catalog: &Catalog, prepared: &Prepared) -> DuckDbLoaded {
        DuckDb::load(&settings(1), catalog, prepared).expect("DuckDB loads")
    }

    /// The value of one of the connection's settings, as text.
    fn setting(loaded: &DuckDbLoaded, name: &str) -> String {
        loaded
            .connection
            .query_row(
                &format!("SELECT CAST(current_setting('{name}') AS VARCHAR)"),
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("a setting")
    }

    /// How many spec tables the connection holds.
    fn spec_tables(loaded: &DuckDbLoaded) -> i64 {
        loaded
            .connection
            .query_row(
                "SELECT count(*) FROM duckdb_tables() WHERE table_name LIKE 'idx%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("a count")
    }

    #[test]
    fn the_engine_is_named_by_its_settings() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        assert_eq!(DuckDbSettings::default().threads, 1);
        assert_eq!(DuckDb::CAPABILITIES, EngineKind::Duckdb.capabilities());
        assert_eq!(
            DuckDb::CAPABILITIES.label(DuckDb::threads(&DuckDbSettings::default())),
            "duckdb-t1"
        );
        assert_eq!(DuckDb::threads(&settings(4)), 4);
        assert!(DuckDb::supports(&prepared).is_ok());
    }

    #[test]
    fn a_zero_thread_count_is_refused() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let Err(error) = DuckDb::load(&settings(0), &catalog, &prepared) else {
            panic!("a thread count of zero must be refused");
        };
        assert_eq!(error.engine, "duckdb");
        assert!(error.message.contains("at least 1"), "{error}");
    }

    #[test]
    fn a_catalog_without_the_relation_is_an_engine_error() {
        let with = graph();
        let prepared = prepared(&with, TRIANGLE);
        let without = catalog(vec![("other", 1, vec![vec![1]])]);
        let Err(error) = DuckDb::load(&settings(1), &without, &prepared) else {
            panic!("a catalog without the relation must be refused");
        };
        assert_eq!(error.engine, "duckdb");
        assert!(error.message.contains("`e`"), "{error}");
    }

    #[test]
    fn the_library_version_is_the_one_that_runs() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let loaded = load(&catalog, &prepared);
        let version = DuckDb::version(&loaded);
        assert!(
            version.starts_with("v1.4"),
            "unexpected DuckDB version {version}"
        );
    }

    #[test]
    fn a_connection_gets_its_threads_a_memory_limit_and_a_private_spill_directory() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let parent = tempfile::tempdir().expect("a scratch directory");
        let loaded = DuckDb::load(
            &DuckDbSettings {
                threads: 3,
                spill_parent: parent.path().to_path_buf(),
            },
            &catalog,
            &prepared,
        )
        .expect("DuckDB loads");
        assert_eq!(setting(&loaded, "threads"), "3");
        // 8GB is about 7.5 GiB, which DuckDB prints in binary units.
        let memory_limit = setting(&loaded, "memory_limit");
        assert!(
            memory_limit.starts_with("7.") && memory_limit.ends_with("GiB"),
            "memory_limit is {memory_limit}"
        );
        let spill = &loaded._spill.path;
        assert_eq!(
            spill.parent(),
            Some(parent.path()),
            "under the settings' parent"
        );
        let directory = spill
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a UTF-8 directory name");
        assert!(
            directory.starts_with(&format!("kermit-duckdb-{}-", std::process::id())),
            "unexpected spill directory {directory}"
        );
        assert!(
            setting(&loaded, "temp_directory").contains(directory),
            "temp_directory does not name {directory}"
        );
    }

    /// On disk beside the benchmark cache, as the JOB generator's raw cache
    /// keeps its own `duckdb.tmp`, and not the system's temporary directory,
    /// which can be a tmpfs (memory).
    #[test]
    fn the_default_spill_parent_is_beside_the_benchmark_cache() {
        let benchmarks = kermit_bench::cache::base_cache_dir().expect("a platform cache directory");
        let kermit = benchmarks.parent().expect("the cache has a parent");
        assert_eq!(
            DuckDbSettings::default().spill_parent,
            kermit.join("duckdb.tmp")
        );
    }

    /// A spill directory is made under its parent, which is made if missing,
    /// and its lock is held for as long as the guard lives.
    #[test]
    fn a_spill_directory_is_made_under_its_parent_and_holds_its_lock() {
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let parent = scratch.path().join("cache").join("duckdb.tmp");
        let spill = SpillDirectory::create(&parent).expect("a spill directory");
        assert_eq!(spill.path.parent(), Some(parent.as_path()));
        let lock = File::open(spill.path.join(LOCK_FILE)).expect("the lock file");
        assert!(lock.try_lock().is_err(), "a live directory's lock is taken");
        let path = spill.path.clone();
        drop(spill);
        assert!(!path.exists(), "the directory outlived its guard");
    }

    /// A directory whose lock can be taken was left by a process that died
    /// without removing it (a killed process runs no destructor, but the OS
    /// frees its locks): the next spill directory made beside it removes it,
    /// spill files and all.
    #[test]
    fn a_spill_directory_its_owner_abandoned_is_removed() {
        let parent = tempfile::tempdir().expect("a scratch directory");
        let abandoned = parent.path().join("kermit-duckdb-1-0");
        fs::create_dir(&abandoned).unwrap();
        File::create(abandoned.join(LOCK_FILE)).unwrap();
        fs::write(abandoned.join("duckdb_temp_storage-0.tmp"), b"spill").unwrap();
        let _spill = SpillDirectory::create(parent.path()).expect("a spill directory");
        assert!(
            !abandoned.exists(),
            "the abandoned directory is still there"
        );
    }

    /// A directory whose lock is taken belongs to a live connection (here, one
    /// of this process's): it is never touched. Nor is a spill directory with
    /// no lock file yet (one being made), or a directory of another name.
    #[test]
    fn a_live_or_unknown_directory_is_kept() {
        let parent = tempfile::tempdir().expect("a scratch directory");
        let live = SpillDirectory::create(parent.path()).expect("a spill directory");
        let being_made = parent.path().join("kermit-duckdb-2-0");
        fs::create_dir(&being_made).unwrap();
        let other = parent.path().join("someone-else");
        fs::create_dir(&other).unwrap();
        File::create(other.join(LOCK_FILE)).unwrap();
        let _next = SpillDirectory::create(parent.path()).expect("a second spill directory");
        assert!(live.path.is_dir(), "the live directory was removed");
        assert!(being_made.is_dir(), "the directory being made was removed");
        assert!(other.is_dir(), "another program's directory was removed");
    }

    /// A name already taken (by a dead process that had this process's id) is
    /// skipped for the next number, and not deleted: only a lock that can be
    /// taken shows that a directory was abandoned.
    #[test]
    fn a_taken_name_is_skipped_not_deleted() {
        let parent = tempfile::tempdir().expect("a scratch directory");
        let name = |sequence: usize| format!("kermit-duckdb-{}-{sequence}", std::process::id());
        let taken = parent.path().join(name(0));
        fs::create_dir(&taken).unwrap();
        fs::write(taken.join("left-behind"), b"").unwrap();
        let mut sequence = 0..;
        let spill = SpillDirectory::create_with(parent.path(), || sequence.next().unwrap())
            .expect("a spill directory");
        assert_eq!(spill.path, parent.path().join(name(1)));
        assert!(
            taken.join("left-behind").exists(),
            "the taken directory was deleted"
        );
    }

    #[test]
    fn each_load_has_its_own_spill_directory_and_removes_it_on_drop() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let first = load(&catalog, &prepared);
        let second = load(&catalog, &prepared);
        let (first_path, second_path) = (first._spill.path.clone(), second._spill.path.clone());
        assert_ne!(first_path, second_path);
        assert!(first_path.is_dir() && second_path.is_dir());
        drop(first);
        assert!(
            !first_path.exists(),
            "the first directory outlived its load"
        );
        assert!(
            second_path.is_dir(),
            "dropping one load removed the other's directory"
        );
        drop(second);
        assert!(
            !second_path.exists(),
            "the second directory outlived its load"
        );
    }

    #[test]
    fn stream_counts_the_bag_and_materialize_the_distinct_rows() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let loaded = load(&catalog, &prepared);
        let built = DuckDb::build(&loaded, &prepared).expect("DuckDB builds");
        assert_eq!(DuckDb::stream(&built).expect("streams"), 2);
        assert_eq!(
            DuckDb::stream(&built).expect("streams again"),
            2,
            "the prepared statement runs again"
        );
        let (distinct, kept) = DuckDb::materialize(&built).expect("materializes");
        assert_eq!(distinct, 1);
        assert_eq!(
            kept.expect("DuckDB returns its sorted batch").to_vecs(),
            vec![vec![1, 2, 3], vec![1, 2, 3]],
            "the batch is the sorted bag, not the distinct rows"
        );
        let mut rows = DuckDb::rows(&built).expect("returns rows");
        rows.sort();
        assert_eq!(rows, vec![vec![1, 2, 3], vec![1, 2, 3]]);
        assert_eq!(DuckDb::space(&built), None);
    }

    /// Keys above `i64::MAX` survive the base table's `UBIGINT` columns, a
    /// constant filter on one, a join on one, and the conversion back to
    /// `usize`.
    #[test]
    fn keys_above_i64_max_load_filter_join_and_come_back_exactly() {
        const HIGH: usize = 1 << 63;
        let catalog = catalog(vec![
            ("e", 2, vec![
                vec![usize::MAX, 1],
                vec![HIGH, 2],
                vec![5, 3],
                vec![HIGH, 4],
            ]),
            ("f", 2, vec![vec![usize::MAX, 7], vec![HIGH, 8], vec![6, 9]]),
        ]);
        let rows_of = |text: &str| {
            let prepared = prepared(&catalog, text);
            let loaded = load(&catalog, &prepared);
            let built = DuckDb::build(&loaded, &prepared).expect("DuckDB builds");
            let mut rows = DuckDb::rows(&built).expect("returns rows");
            rows.sort();
            rows
        };
        assert_eq!(
            rows_of("Q(X) :- e(X, c4)."),
            vec![vec![HIGH]],
            "a constant filter on the second column"
        );
        assert_eq!(
            rows_of("Q(Y) :- e(c9223372036854775808, Y)."),
            vec![vec![2], vec![4]],
            "a constant above i64::MAX"
        );
        assert_eq!(
            rows_of(&format!("Q(Y) :- e(c{}, Y).", usize::MAX)),
            vec![vec![1]],
            "usize::MAX as a constant"
        );
        assert_eq!(
            rows_of("Q(K, A, B) :- e(K, A), f(K, B)."),
            vec![vec![HIGH, 2, 8], vec![HIGH, 4, 8], vec![usize::MAX, 1, 7]],
            "a join on the large key"
        );
    }

    #[test]
    fn builds_of_one_load_coexist_and_a_dropped_build_takes_its_tables_with_it() {
        let catalog = graph();
        let prepared = prepared(&catalog, TRIANGLE);
        let loaded = load(&catalog, &prepared);
        assert_eq!(spec_tables(&loaded), 0, "loading builds no spec table");
        let first = DuckDb::build(&loaded, &prepared).expect("DuckDB builds");
        let second = DuckDb::build(&loaded, &prepared).expect("DuckDB builds again");
        assert_eq!(spec_tables(&loaded), 2);
        drop(first);
        assert_eq!(spec_tables(&loaded), 1);
        assert_eq!(
            DuckDb::stream(&second).expect("the second build still runs"),
            2
        );
        drop(second);
        assert_eq!(spec_tables(&loaded), 0);
    }
}
