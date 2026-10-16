//! The raw IMDb cache: `imdb.duckdb` (the 21 IMDb tables), `queries/` (the
//! pinned JOB query files) and `provenance.json`, shared by every template.
//!
//! [`build`] loads a cache from a directory of CSVs in a staging directory
//! beside the target and renames it into place, with `provenance.json`
//! written last: a directory that has it is a finished cache, and one that
//! lacks it is never reused. [`RawCache::open`] is the only reader.

use {
    crate::job::duckdb::{path_literal, sql_identifier, Duckdb},
    anyhow::{anyhow, ensure, Context},
    serde::{Deserialize, Serialize},
    std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    },
};

/// The directory name of the raw cache under `<platform cache>/kermit/`.
pub const RAW_DIR_NAME: &str = "imdb";

const DATABASE_FILE: &str = "imdb.duckdb";
const QUERIES_DIR: &str = "queries";
const TEMP_DIR: &str = "duckdb.tmp";
const PROVENANCE_FILE: &str = "provenance.json";

/// `<platform cache>/kermit/imdb`, beside the `benchmarks` directory that
/// `kermit_bench::cache::base_cache_dir` names.
///
/// # Errors
///
/// Fails when the platform has no cache directory.
pub fn default_raw_dir() -> anyhow::Result<PathBuf> {
    let benchmarks = kermit_bench::cache::base_cache_dir()?;
    let kermit_cache = benchmarks
        .parent()
        .with_context(|| format!("{} has no parent directory", benchmarks.display()))?;
    Ok(kermit_cache.join(RAW_DIR_NAME))
}

/// What a raw cache was built from; `provenance.json` holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawProvenance {
    /// SHA-256 of the `imdb.tgz` the tables were loaded from.
    pub imdb_sha256: String,
    /// The commit of `gregrahn/join-order-benchmark` the queries were read at.
    pub query_commit: String,
    /// SHA-256 of each query file (and `schema.sql`), by file name.
    pub query_sha256: BTreeMap<String, String>,
    /// `duckdb --version` of the build.
    pub duckdb_version: String,
}

/// Everything [`build`] loads a raw cache from.
pub struct BuildInputs<'a> {
    /// A directory holding `<table>.csv` for every table `schema_sql` creates.
    pub csv_dir: &'a Path,
    /// The `schema.sql` that creates the tables.
    pub schema_sql: &'a Path,
    /// A directory holding only the query files, `<template><letters>.sql`
    /// (`16b.sql`); they are copied into the cache.
    pub queries_dir: &'a Path,
    /// The row count each table must have after loading.
    pub table_rows: &'a [(&'a str, u64)],
    /// Written to `provenance.json` once everything else is in place.
    pub provenance: RawProvenance,
}

/// A complete raw cache: `imdb.duckdb`, `queries/` and `provenance.json`
/// (written last).
#[derive(Debug, Clone)]
pub struct RawCache {
    dir: PathBuf,
}

impl RawCache {
    /// The cache at `dir`, if it is finished: `Some` exactly when
    /// `<dir>/provenance.json` exists, since [`build`] writes it last.
    pub fn open(dir: &Path) -> Option<RawCache> {
        dir.join(PROVENANCE_FILE).is_file().then(|| RawCache {
            dir: dir.to_path_buf(),
        })
    }

    /// The cache directory.
    pub fn dir(&self) -> &Path { &self.dir }

    /// `<dir>/imdb.duckdb`.
    pub fn database(&self) -> PathBuf { self.dir.join(DATABASE_FILE) }

    /// `<dir>/duckdb.tmp`, the parent of each DuckDB call's own spill
    /// directory on this cache.
    pub fn temp_dir(&self) -> PathBuf { self.dir.join(TEMP_DIR) }

    /// What the cache was built from.
    ///
    /// # Errors
    ///
    /// Fails if `provenance.json` cannot be read or parsed.
    pub fn provenance(&self) -> anyhow::Result<RawProvenance> {
        let path = self.dir.join(PROVENANCE_FILE);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// `(name, sql)` of `<dir>/queries/<template><letters>.sql` for every
    /// variant of `template`, in letter order (`("1a", …)`, `("1b", …)`).
    /// Empty when the cache holds none; other files in `queries/` are ignored,
    /// and `1` never matches `10a`.
    ///
    /// # Errors
    ///
    /// Fails if `queries/` or a query file cannot be read.
    pub fn template_queries(&self, template: u8) -> anyhow::Result<Vec<(String, String)>> {
        let dir = self.dir.join(QUERIES_DIR);
        let mut found = Vec::new();
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry
                .with_context(|| format!("reading {}", dir.display()))?
                .path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("sql") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Some((number, letters)) = split_query_stem(stem) else {
                continue;
            };
            if number == u32::from(template) {
                found.push((letters.to_owned(), path.clone()));
            }
        }
        found.sort_by_key(|(letters, _)| (letters.len(), letters.clone()));
        found
            .into_iter()
            .map(|(letters, path)| -> anyhow::Result<(String, String)> {
                let sql = fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                Ok((format!("{template}{letters}"), sql))
            })
            .collect()
    }
}

/// Splits a query file stem such as `16b` into its template number and its
/// variant letters; `None` for anything else (`schema`, `1`, `1A`, `1a2`,
/// `01a`: a leading zero would give `1a` two file names).
pub(crate) fn split_query_stem(stem: &str) -> Option<(u32, &str)> {
    let digits = stem.find(|c: char| !c.is_ascii_digit())?;
    let (number, letters) = stem.split_at(digits);
    if number.is_empty()
        || number.starts_with('0')
        || !letters.chars().all(|c| c.is_ascii_lowercase())
    {
        return None;
    }
    Some((number.parse().ok()?, letters))
}

/// A staging directory that removes itself on drop unless it was renamed
/// into place. It is the one staging guard of the raw cache: [`build`] stages
/// in `<dir>.building-<pid>-<seq>`, and the download stage of `ensure` stages
/// in `<dir>.fetch-<pid>` with the same type.
///
/// `Drop` runs on every `?` return and on a panic. A process killed by a
/// signal (`SIGINT`, `SIGKILL`) skips it, so a directory under a dead pid can
/// remain; nothing cleans those up automatically.
struct Staging {
    path: Option<PathBuf>,
}

impl Staging {
    /// Creates `path` empty, and its parent directory if that is missing (a
    /// fresh `$XDG_CACHE_HOME/kermit` has no `imdb` yet), first removing
    /// whatever a killed run left at `path`.
    fn create(path: PathBuf) -> anyhow::Result<Staging> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        if path.exists() {
            fs::remove_dir_all(&path).with_context(|| {
                format!("removing the stale staging directory {}", path.display())
            })?;
        }
        fs::create_dir(&path)
            .with_context(|| format!("creating the staging directory {}", path.display()))?;
        Ok(Staging {
            path: Some(path),
        })
    }

    fn path(&self) -> &Path { self.path.as_deref().expect("a live staging directory") }

    /// The directory has been renamed into place: nothing to clean up.
    fn renamed(mut self) { self.path = None; }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Builds a raw cache in `<dir>.building-<pid>-<seq>` and renames it to `dir`.
/// (`seq` is a process-wide counter, so two builds of one `dir` in one process
/// never share a staging directory.)
///
/// Loads `schema.sql`, then each table the schema created (read back from
/// `information_schema.tables`) with `COPY <table> FROM '<csv>' (FORMAT csv,
/// HEADER false, DELIMITER ',', QUOTE '"', ESCAPE '\', ALLOW_QUOTED_NULLS
/// false, AUTO_DETECT false)`, checking every row count against
/// `inputs.table_rows`, copies the queries and writes `provenance.json` last.
/// The dialect is pinned to PostgreSQL CSV's, which wrote the dump: left to
/// DuckDB's detection, a quoted empty `""` (an empty string in PostgreSQL)
/// loads as NULL, and a sample of fields that all happen to be `'x'` makes it
/// pick the apostrophe as the quote.
/// A table without its CSV or without a pinned count, a count that differs,
/// or a query file that is not `<template><letters>.sql` is an error naming
/// it, and leaves no `dir` and no staging directory (a process killed by a
/// signal leaves its staging directory behind). `dir`'s parent directories are
/// created if missing. If `dir` exists at the rename and holds a finished
/// cache, the staging directory is removed and that cache is returned; if it
/// exists and holds none, the build fails and leaves `dir` as it was.
///
/// # Errors
///
/// As described above, and on any DuckDB or file-system failure.
pub fn build(duckdb: &Duckdb, dir: &Path, inputs: &BuildInputs<'_>) -> anyhow::Result<RawCache> {
    let parent = match dir.parent() {
        | Some(parent) if !parent.as_os_str().is_empty() => parent,
        | _ => Path::new("."),
    };
    let name = dir
        .file_name()
        .with_context(|| format!("{} names no directory", dir.display()))?
        .to_string_lossy()
        .into_owned();
    let staging = Staging::create(staging_path(parent, &name))?;
    load_into(duckdb, staging.path(), inputs)?;
    match fs::rename(staging.path(), dir) {
        | Ok(()) => {
            staging.renamed();
            Ok(RawCache {
                dir: dir.to_path_buf(),
            })
        },
        // Another build finished first: keep its cache, drop ours.
        | Err(error) => match RawCache::open(dir) {
            | Some(winner) => Ok(winner),
            | None => Err(anyhow!(error).context(format!(
                "moving {} to {}",
                staging.path().display(),
                dir.display()
            ))),
        },
    }
}

/// `<parent>/<name>.building-<pid>-<seq>`, with `seq` the next value of a
/// process-wide counter.
fn staging_path(parent: &Path, name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{name}.building-{}-{seq}", std::process::id()))
}

/// Fills `staging`: database, queries, then `provenance.json`.
fn load_into(duckdb: &Duckdb, staging: &Path, inputs: &BuildInputs<'_>) -> anyhow::Result<()> {
    let database = staging.join(DATABASE_FILE);
    let temp = staging.join(TEMP_DIR);

    let schema = fs::read_to_string(inputs.schema_sql)
        .with_context(|| format!("reading {}", inputs.schema_sql.display()))?;
    duckdb
        .execute(&database, &temp, &schema)
        .with_context(|| format!("loading {}", inputs.schema_sql.display()))?;
    let tables = duckdb
        .query_strings(
            &database,
            &temp,
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'main' AND \
             table_type = 'BASE TABLE' ORDER BY table_name",
        )
        .context("listing the tables the schema created")?;
    ensure!(
        !tables.is_empty(),
        "{} created no tables",
        inputs.schema_sql.display()
    );

    // Check every table has its CSV and its pinned count before loading any.
    let mut plan = Vec::with_capacity(tables.len());
    for table in &tables {
        let csv = inputs.csv_dir.join(format!("{table}.csv"));
        ensure!(
            csv.is_file(),
            "table `{table}` has no CSV at {}",
            csv.display()
        );
        let expected = inputs
            .table_rows
            .iter()
            .find(|(name, _)| *name == table.as_str())
            .map(|(_, rows)| *rows)
            .with_context(|| format!("table `{table}` has no pinned row count"))?;
        plan.push((table, csv, expected));
    }
    for (table, csv, expected) in plan {
        let copy = format!(
            "COPY {} FROM {} (FORMAT csv, HEADER false, DELIMITER ',', QUOTE '\"', ESCAPE '\\', \
             ALLOW_QUOTED_NULLS false, AUTO_DETECT false)",
            sql_identifier(table),
            path_literal(&csv)?
        );
        duckdb
            .execute(&database, &temp, &copy)
            .with_context(|| format!("loading table `{table}` from {}", csv.display()))?;
        let loaded = duckdb
            .query_u64(
                &database,
                &temp,
                &format!("SELECT COUNT(*) FROM {}", sql_identifier(table)),
            )
            .with_context(|| format!("counting table `{table}`"))?;
        ensure!(
            loaded == expected,
            "table `{table}` loaded {loaded} rows, expected {expected}"
        );
    }

    copy_queries(inputs.queries_dir, &staging.join(QUERIES_DIR))?;

    let provenance =
        serde_json::to_string_pretty(&inputs.provenance).context("serialising the provenance")?;
    let path = staging.join(PROVENANCE_FILE);
    fs::write(&path, format!("{provenance}\n"))
        .with_context(|| format!("writing {}", path.display()))
}

/// Copies every query file of `from` into the new directory `to`; each must
/// be a regular file named `<template><letters>.sql`.
fn copy_queries(from: &Path, to: &Path) -> anyhow::Result<()> {
    fs::create_dir(to).with_context(|| format!("creating {}", to.display()))?;
    let mut files = Vec::new();
    for entry in fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        files.push(
            entry
                .with_context(|| format!("reading {}", from.display()))?
                .path(),
        );
    }
    files.sort();
    ensure!(!files.is_empty(), "{} holds no query files", from.display());
    for file in files {
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let is_query = file.is_file()
            && name
                .strip_suffix(".sql")
                .and_then(split_query_stem)
                .is_some();
        ensure!(
            is_query,
            "{} is not a query file named <template><letters>.sql",
            file.display()
        );
        fs::copy(&file, to.join(name)).with_context(|| format!("copying {}", file.display()))?;
    }
    Ok(())
}

/// Unpacks `tgz` into the directory `into` (created if missing) with
/// `tar -xzf <tgz> -C <into>`.
///
/// # Errors
///
/// Fails if `tar` cannot run or exits non-zero; the message quotes its stderr.
pub fn extract_tgz(tgz: &Path, into: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(into).with_context(|| format!("creating {}", into.display()))?;
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(tgz)
        .arg("-C")
        .arg(into)
        .output()
        .context("running `tar`")?;
    ensure!(
        output.status.success(),
        "`tar -xzf {}` failed ({}): {}",
        tgz.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

/// The IMDb raw cache at `dir`, if a finished one is there.
///
/// This is the stand-in until the download lands (#97's last phase): it opens
/// the cache or fails, and never builds one. The failure says the cache is
/// missing; [`build`] makes one from a directory of CSVs. The signature is
/// the final one, so callers do not change when the download replaces the
/// body.
///
/// # Errors
///
/// Fails when `dir` holds no finished cache (no `provenance.json`).
pub fn ensure(_duckdb: &Duckdb, dir: &Path) -> anyhow::Result<RawCache> {
    RawCache::open(dir).ok_or_else(|| {
        anyhow!(
            "the IMDb raw cache at {} is missing (no finished cache with {PROVENANCE_FILE}); \
             downloading it arrives with the next phase of #97 (J7)",
            dir.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use {super::*, crate::job::duckdb::test_support::locate_or_skip};

    /// The committed fixture: `schema.sql`, `csv/`, `queries/` and `rows.txt`.
    fn fixture_dir() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/job") }

    /// `rows.txt`: one `<table> <rows>` pair per line.
    fn fixture_rows() -> Vec<(String, u64)> {
        let text = fs::read_to_string(fixture_dir().join("rows.txt")).unwrap();
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let (table, rows) = line.split_once(' ').unwrap();
                (table.to_owned(), rows.parse().unwrap())
            })
            .collect()
    }

    fn provenance(duckdb_version: &str) -> RawProvenance {
        RawProvenance {
            imdb_sha256: "ab".repeat(32),
            query_commit: "a39603662e023e449cb2121997a5034df9e02ebf".to_owned(),
            query_sha256: BTreeMap::from([("1a.sql".to_owned(), "cd".repeat(32))]),
            duckdb_version: duckdb_version.to_owned(),
        }
    }

    /// Builds the fixture's queries and `rows` from `csv_dir` into `dir`.
    fn build_from(
        duckdb: &Duckdb, dir: &Path, csv_dir: &Path, rows: &[(String, u64)], version: &str,
    ) -> anyhow::Result<RawCache> {
        build_with_schema(
            duckdb,
            dir,
            &fixture_dir().join("schema.sql"),
            csv_dir,
            rows,
            version,
        )
    }

    /// [`build_from`] over a schema other than the fixture's.
    fn build_with_schema(
        duckdb: &Duckdb, dir: &Path, schema_sql: &Path, csv_dir: &Path, rows: &[(String, u64)],
        version: &str,
    ) -> anyhow::Result<RawCache> {
        let queries_dir = fixture_dir().join("queries");
        let table_rows: Vec<(&str, u64)> =
            rows.iter().map(|(table, n)| (table.as_str(), *n)).collect();
        build(duckdb, dir, &BuildInputs {
            csv_dir,
            schema_sql,
            queries_dir: &queries_dir,
            table_rows: &table_rows,
            provenance: provenance(version),
        })
    }

    fn build_fixture(duckdb: &Duckdb, dir: &Path, version: &str) -> anyhow::Result<RawCache> {
        build_from(
            duckdb,
            dir,
            &fixture_dir().join("csv"),
            &fixture_rows(),
            version,
        )
    }

    /// The statement that counts a fixture query: its projection replaced by
    /// `COUNT(*)`. (The generator derives this from the parsed AST; the test
    /// needs no translator.)
    fn count_sql(sql: &str) -> String {
        let from = sql
            .find("\nFROM")
            .expect("every fixture query has a FROM line");
        format!(
            "SELECT COUNT(*){}",
            sql[from..].trim_end().trim_end_matches(';')
        )
    }

    /// A hand-made cache directory (no DuckDB): `queries/` holding `files`
    /// and a `provenance.json`.
    fn hand_made_cache(root: &Path, files: &[&str]) -> RawCache {
        let dir = root.join("imdb");
        fs::create_dir_all(dir.join(QUERIES_DIR)).unwrap();
        for file in files {
            fs::write(dir.join(QUERIES_DIR).join(file), format!("-- {file}\n")).unwrap();
        }
        let json = serde_json::to_string_pretty(&provenance("v")).unwrap();
        fs::write(dir.join(PROVENANCE_FILE), json).unwrap();
        RawCache::open(&dir).unwrap()
    }

    // ---- no DuckDB needed ---------------------------------------------------

    #[test]
    fn split_query_stem_takes_template_and_letters() {
        assert_eq!(split_query_stem("1a"), Some((1, "a")));
        assert_eq!(split_query_stem("10a"), Some((10, "a")));
        assert_eq!(split_query_stem("33c"), Some((33, "c")));
        for bad in [
            "", "1", "a1", "schema", "1A", "1a2", "x1a", "01a", "0a", "001b",
        ] {
            assert_eq!(split_query_stem(bad), None, "{bad:?}");
        }
    }

    /// Variants come back in letter order, a template never picks up another
    /// whose number starts with its own (`1` and `10a`), and files that are
    /// not `<template><letters>.sql` are ignored.
    #[test]
    fn template_queries_are_in_letter_order() {
        let scratch = tempfile::tempdir().unwrap();
        let cache = hand_made_cache(scratch.path(), &[
            "1b.sql",
            "10a.sql",
            "1a.sql",
            "1c.sql",
            "2a.sql",
            "notes.txt",
            "1.sql",
            "schema.sql",
        ]);
        let names = |template: u8| -> Vec<String> {
            cache
                .template_queries(template)
                .unwrap()
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        };
        assert_eq!(names(1), ["1a", "1b", "1c"]);
        assert_eq!(names(10), ["10a"]);
        assert_eq!(names(2), ["2a"]);
        assert!(names(3).is_empty());
        let first = cache.template_queries(1).unwrap().remove(0);
        assert_eq!(first, ("1a".to_owned(), "-- 1a.sql\n".to_owned()));
    }

    /// A folder holding something that is not `<template><letters>.sql`
    /// (`schema.sql`, `1A.sql`, a directory named like a query) is refused
    /// naming it.
    #[test]
    fn copy_queries_refuses_what_is_not_a_query_file() {
        let scratch = tempfile::tempdir().unwrap();
        for (index, bad) in ["schema.sql", "1A.sql", "notes.txt", "1a.sql.bak"]
            .into_iter()
            .enumerate()
        {
            let from = scratch.path().join(format!("from{index}"));
            fs::create_dir(&from).unwrap();
            fs::write(from.join("1a.sql"), "-- 1a\n").unwrap();
            fs::write(from.join(bad), "-- bad\n").unwrap();
            let error =
                copy_queries(&from, &scratch.path().join(format!("to{index}"))).unwrap_err();
            let message = format!("{error:#}");
            assert!(
                message.contains(bad) && message.contains("is not a query file"),
                "{bad}: {message}"
            );
        }
        // A directory named like a query is not a regular file.
        let from = scratch.path().join("from-dir");
        fs::create_dir_all(from.join("2a.sql")).unwrap();
        let error = copy_queries(&from, &scratch.path().join("to-dir")).unwrap_err();
        assert!(
            format!("{error:#}").contains("is not a query file"),
            "{error:#}"
        );
        // A good folder copies.
        let from = scratch.path().join("from-good");
        fs::create_dir(&from).unwrap();
        fs::write(from.join("1a.sql"), "-- 1a\n").unwrap();
        let to = scratch.path().join("to-good");
        copy_queries(&from, &to).unwrap();
        assert_eq!(fs::read_to_string(to.join("1a.sql")).unwrap(), "-- 1a\n");
    }

    #[test]
    fn copy_queries_refuses_an_empty_folder() {
        let scratch = tempfile::tempdir().unwrap();
        let from = scratch.path().join("from");
        fs::create_dir(&from).unwrap();
        let error = copy_queries(&from, &scratch.path().join("to")).unwrap_err();
        assert!(
            format!("{error:#}").contains("holds no query files"),
            "{error:#}"
        );
    }

    /// Two staging paths for one `dir` in one process differ, and both name
    /// the pid.
    #[test]
    fn staging_paths_are_distinct_within_a_process() {
        let parent = Path::new("/cache");
        let first = staging_path(parent, "imdb");
        let second = staging_path(parent, "imdb");
        assert_ne!(first, second);
        let prefix = format!("imdb.building-{}-", std::process::id());
        for path in [&first, &second] {
            assert_eq!(path.parent(), Some(parent));
            assert!(
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&prefix),
                "{}",
                path.display()
            );
        }
    }

    /// Whatever a killed run left at the staging path is removed, never reused
    /// (not even a directory that looks finished).
    #[test]
    fn staging_create_removes_a_leftover() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("imdb.building-1-0");
        fs::create_dir_all(path.join(QUERIES_DIR)).unwrap();
        fs::write(path.join("stale.txt"), "left by a killed build").unwrap();
        fs::write(path.join(PROVENANCE_FILE), "{}").unwrap();
        let staging = Staging::create(path.clone()).unwrap();
        assert_eq!(fs::read_dir(staging.path()).unwrap().count(), 0);
        drop(staging);
        assert!(!path.exists(), "dropped, so removed");
    }

    #[test]
    fn provenance_round_trips() {
        let scratch = tempfile::tempdir().unwrap();
        let cache = hand_made_cache(scratch.path(), &["1a.sql"]);
        assert_eq!(cache.provenance().unwrap(), provenance("v"));
        // A corrupt file is an error naming it, not a default.
        fs::write(cache.dir().join(PROVENANCE_FILE), "not json").unwrap();
        let error = cache.provenance().unwrap_err();
        assert!(format!("{error:#}").contains(PROVENANCE_FILE), "{error:#}");
    }

    /// `provenance.json` is the commit marker: a directory with a database and
    /// queries but no provenance is an unfinished cache.
    #[test]
    fn open_needs_provenance_last() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        fs::create_dir_all(dir.join(QUERIES_DIR)).unwrap();
        fs::write(dir.join(DATABASE_FILE), b"").unwrap();
        assert!(RawCache::open(&dir).is_none());
        fs::write(dir.join(PROVENANCE_FILE), "{}").unwrap();
        assert!(RawCache::open(&dir).is_some());
        assert!(RawCache::open(&scratch.path().join("absent")).is_none());
    }

    #[test]
    fn default_raw_dir_sits_beside_the_benchmark_cache() {
        let (Ok(raw), Ok(benchmarks)) = (default_raw_dir(), kermit_bench::cache::base_cache_dir())
        else {
            eprintln!("skipping: no platform cache directory");
            return;
        };
        assert_eq!(raw.file_name().unwrap(), RAW_DIR_NAME);
        assert_eq!(raw.parent(), benchmarks.parent());
    }

    #[test]
    fn extract_tgz_unpacks_at_the_top_level() {
        let scratch = tempfile::tempdir().unwrap();
        let source = scratch.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("title.csv"), "1,a\n").unwrap();
        let tgz = scratch.path().join("x.tgz");
        let made = Command::new("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(&source)
            .arg("title.csv")
            .status();
        let Ok(status) = made else {
            eprintln!("skipping: no tar");
            return;
        };
        assert!(status.success());
        let into = scratch.path().join("out");
        extract_tgz(&tgz, &into).unwrap();
        assert_eq!(fs::read_to_string(into.join("title.csv")).unwrap(), "1,a\n");
    }

    #[test]
    fn extract_tgz_rejects_a_file_that_is_not_a_tgz() {
        // Without `tar`, `extract_tgz` fails before it names the file.
        if Command::new("tar").arg("--version").output().is_err() {
            eprintln!("skipping: no tar");
            return;
        }
        let scratch = tempfile::tempdir().unwrap();
        let tgz = scratch.path().join("garbage.tgz");
        fs::write(&tgz, "this is not gzip").unwrap();
        let error = extract_tgz(&tgz, &scratch.path().join("out")).unwrap_err();
        assert!(format!("{error:#}").contains("garbage.tgz"), "{error:#}");
    }

    // ---- DuckDB-gated -------------------------------------------------------

    /// The fixture loads: every table has its pinned row count, the
    /// backslash-escaped quotes arrived as quotes with the other fields in
    /// place, the NULLs stayed NULL, 2b's filter matches nothing, the queries
    /// were copied, and nothing is left beside the cache.
    #[test]
    fn builds_the_fixture() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let cache = build_fixture(&duckdb, &dir, "v-test").unwrap();
        assert_eq!(cache.dir(), dir.as_path());
        assert!(cache.database().is_file());
        assert!(RawCache::open(&dir).is_some());
        assert_eq!(cache.provenance().unwrap(), provenance("v-test"));
        let count = |sql: &str| {
            duckdb
                .query_u64(&cache.database(), &cache.temp_dir(), sql)
                .unwrap()
        };
        for (table, rows) in fixture_rows() {
            assert_eq!(
                count(&format!("SELECT COUNT(*) FROM \"{table}\"")),
                rows,
                "{table}"
            );
        }
        assert_eq!(
            count(r#"SELECT COUNT(*) FROM title WHERE title LIKE '%"Hero"%'"#),
            2
        );
        assert_eq!(
            count("SELECT production_year FROM title WHERE id = 2"),
            1995
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM title WHERE title = 'Hero, Interrupted'"),
            1
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM title WHERE production_year IS NULL"),
            1
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM cast_info WHERE person_id IS NULL"),
            1
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM movie_info WHERE info_type_id IS NULL"),
            1
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM name WHERE name LIKE '%Zed%'"),
            0
        );
        for query in ["1a.sql", "1b.sql", "2a.sql", "2b.sql"] {
            assert!(dir.join(QUERIES_DIR).join(query).is_file(), "{query}");
        }
        assert_eq!(
            fs::read_dir(scratch.path()).unwrap().count(),
            1,
            "no staging directory"
        );
    }

    /// The four fixture queries, counted by DuckDB, give the numbers the
    /// fixture-facts table in the plan derives by hand (and that J5 relies on).
    #[test]
    fn fixture_queries_count_as_the_facts_say() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let cache = build_fixture(&duckdb, &scratch.path().join("imdb"), "v-test").unwrap();
        let mut counts = Vec::new();
        for template in [1, 2] {
            for (name, sql) in cache.template_queries(template).unwrap() {
                let rows = duckdb
                    .query_u64(&cache.database(), &cache.temp_dir(), &count_sql(&sql))
                    .unwrap();
                counts.push((name, rows));
            }
        }
        let expected = [("1a", 5), ("1b", 7), ("2a", 2), ("2b", 0)];
        assert_eq!(counts, expected.map(|(name, rows)| (name.to_owned(), rows)),);
    }

    /// A pinned count that disagrees with the loaded table is an error naming
    /// the table and both counts, and leaves neither a cache nor a staging
    /// directory.
    #[test]
    fn row_count_mismatch_leaves_no_cache() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let mut rows = fixture_rows();
        for entry in &mut rows {
            if entry.0 == "title" {
                entry.1 = 10;
            }
        }
        let error = build_from(&duckdb, &dir, &fixture_dir().join("csv"), &rows, "v").unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("table `title` loaded 9 rows, expected 10"),
            "{message}"
        );
        assert!(!dir.exists());
        assert!(RawCache::open(&dir).is_none());
        assert_eq!(
            fs::read_dir(scratch.path()).unwrap().count(),
            0,
            "staging directory removed"
        );
    }

    /// A cache directory whose parent does not exist yet (a fresh
    /// `$XDG_CACHE_HOME/kermit/imdb`) builds: `Staging::create` makes the
    /// parent, and nothing but the cache is left in it.
    #[test]
    fn builds_under_a_parent_that_does_not_exist_yet() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let parent = scratch.path().join("kermit");
        let dir = parent.join("imdb");
        assert!(!parent.exists());
        let cache = build_fixture(&duckdb, &dir, "v-test").unwrap();
        assert_eq!(cache.dir(), dir.as_path());
        assert!(RawCache::open(&dir).is_some());
        assert_eq!(
            fs::read_dir(&parent).unwrap().count(),
            1,
            "no staging directory"
        );
    }

    #[test]
    fn missing_csv_names_the_table() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let csv = scratch.path().join("csv");
        fs::create_dir(&csv).unwrap();
        for entry in fs::read_dir(fixture_dir().join("csv")).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_name().unwrap().to_owned();
            if file != "keyword.csv" {
                fs::copy(&path, csv.join(file)).unwrap();
            }
        }
        let dir = scratch.path().join("imdb");
        let error = build_from(&duckdb, &dir, &csv, &fixture_rows(), "v").unwrap_err();
        assert!(
            format!("{error:#}").contains("table `keyword` has no CSV"),
            "{error:#}"
        );
        assert!(!dir.exists());
    }

    #[test]
    fn missing_row_count_names_the_table() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let mut rows = fixture_rows();
        rows.retain(|(table, _)| table != "keyword");
        let error = build_from(&duckdb, &dir, &fixture_dir().join("csv"), &rows, "v").unwrap_err();
        assert!(
            format!("{error:#}").contains("table `keyword` has no pinned row count"),
            "{error:#}"
        );
        assert!(!dir.exists());
    }

    /// A build that loses the final rename to a concurrent one discards its
    /// own staging directory and adopts the winner's cache.
    #[test]
    fn losing_the_rename_adopts_the_winner() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        let first = build_fixture(&duckdb, &dir, "winner").unwrap();
        let second = build_fixture(&duckdb, &dir, "loser").unwrap();
        assert_eq!(second.dir(), first.dir());
        assert_eq!(second.provenance().unwrap().duckdb_version, "winner");
        assert_eq!(
            fs::read_dir(scratch.path()).unwrap().count(),
            1,
            "the loser's staging is gone"
        );
    }

    /// If `dir` exists at the rename but holds no finished cache, the build
    /// fails, leaves `dir` as it was and removes its staging directory. (A
    /// non-empty directory cannot be renamed over.)
    #[test]
    fn an_unfinished_dir_at_the_rename_is_an_error() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("imdb");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("stray"), "not a cache").unwrap();
        let error = build_fixture(&duckdb, &dir, "v").unwrap_err();
        assert!(format!("{error:#}").contains("moving"), "{error:#}");
        assert!(RawCache::open(&dir).is_none());
        assert_eq!(
            fs::read_to_string(dir.join("stray")).unwrap(),
            "not a cache"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert_eq!(
            fs::read_dir(scratch.path()).unwrap().count(),
            1,
            "no staging directory"
        );
    }

    /// The CSV dialect is PostgreSQL's: a quoted empty field is an empty
    /// string, an unquoted one is NULL, a backslash escapes a quote, and a
    /// field in apostrophes keeps them.
    #[test]
    fn a_quoted_empty_field_loads_as_an_empty_string() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = tempfile::tempdir().unwrap();
        let schema = scratch.path().join("schema.sql");
        fs::write(
            &schema,
            "CREATE TABLE note (id integer NOT NULL, info text, tag text NOT NULL);\n",
        )
        .unwrap();
        let csv = scratch.path().join("csv");
        fs::create_dir(&csv).unwrap();
        fs::write(
            csv.join("note.csv"),
            "1,\"\",a\n2,,b\n3,'x','y'\n4,\"say \\\"hi\\\"\",d\n",
        )
        .unwrap();
        let dir = scratch.path().join("imdb");
        let rows = [("note".to_owned(), 4)];
        let cache = build_with_schema(&duckdb, &dir, &schema, &csv, &rows, "v").unwrap();
        let count = |predicate: &str| {
            duckdb
                .query_u64(
                    &cache.database(),
                    &cache.temp_dir(),
                    &format!("SELECT COUNT(*) FROM note WHERE {predicate}"),
                )
                .unwrap()
        };
        assert_eq!(count("id = 1 AND info IS NOT NULL AND info = ''"), 1);
        assert_eq!(count("id = 2 AND info IS NULL"), 1);
        assert_eq!(count("info IS NULL"), 1);
        assert_eq!(count("id = 3 AND info = '''x''' AND tag = '''y'''"), 1);
        assert_eq!(count("id = 4 AND info = 'say \"hi\"'"), 1);
    }
}
