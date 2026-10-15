//! The IMDb-shaped fixture of `kermit-generate`'s JOB generator
//! (`kermit-generate/tests/fixtures/job/`), built into a raw cache for the
//! DuckDB-gated tests (`job_generate.rs`, `cli_job_gen.rs`).
//!
//! Every cache is built in a directory the caller owns (a temp dir): nothing
//! here touches `~/.cache/kermit`.

use {
    kermit_generate::job::{
        duckdb::Duckdb,
        pins,
        raw::{self, BuildInputs, RawCache, RawProvenance},
    },
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

/// `kermit-generate/tests/fixtures/job/`: `schema.sql`, `csv/`, `queries/`
/// and `rows.txt`.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../kermit-generate/tests/fixtures/job")
}

/// `rows.txt`: the row count each fixture table must load to, one
/// `<table> <rows>` pair per line.
pub fn fixture_rows() -> Vec<(String, u64)> {
    let path = fixture_dir().join("rows.txt");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (table, rows) = line
                .split_once(' ')
                .expect("rows.txt lines are `<table> <rows>`");
            (table.to_owned(), rows.parse().expect("a row count"))
        })
        .collect()
}

/// The located DuckDB, or `None` after a stderr line saying why `test`
/// skipped: CI installs no DuckDB, as it installs no Java for the LUBM tests.
pub fn locate_or_skip(test: &str) -> Option<Duckdb> {
    match Duckdb::locate() {
        | Ok(duckdb) => Some(duckdb),
        | Err(error) => {
            eprintln!("skipping {test}: no DuckDB ({error:#})");
            None
        },
    }
}

/// The directory of fixture CSVs to load: the committed `csv/`, or (when
/// `reversed`) a copy in `scratch` with the lines of every file in reverse
/// order. DuckDB returns a table's rows in load order unless the query sorts
/// them, so only the reversed copy can tell a missing `ORDER BY`. Every
/// fixture record is one line, so reversing the lines reverses the rows.
pub fn csv_dir(scratch: &Path, reversed: bool) -> PathBuf {
    let source = fixture_dir().join("csv");
    if !reversed {
        return source;
    }
    let target = scratch.join("csv-reversed");
    fs::create_dir_all(&target).unwrap();
    for entry in fs::read_dir(&source).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        let mut lines: Vec<&str> = text.lines().collect();
        lines.reverse();
        let mut reversed_text = lines.join("\n");
        reversed_text.push('\n');
        fs::write(target.join(path.file_name().unwrap()), reversed_text).unwrap();
    }
    target
}

/// Builds the raw cache at `dir` from the fixture's `schema.sql` and
/// `rows.txt`, the CSVs in `csv` and the query files in `queries_dir`.
///
/// # Panics
///
/// Panics if the build fails.
pub fn build_raw_cache(
    duckdb: &Duckdb, dir: &Path, csv: &Path, queries_dir: &Path, provenance: RawProvenance,
) -> RawCache {
    let rows = fixture_rows();
    let table_rows: Vec<(&str, u64)> = rows.iter().map(|(table, n)| (table.as_str(), *n)).collect();
    let schema_sql = fixture_dir().join("schema.sql");
    raw::build(duckdb, dir, &BuildInputs {
        csv_dir: csv,
        schema_sql: &schema_sql,
        queries_dir,
        table_rows: &table_rows,
        provenance,
    })
    .unwrap_or_else(|error| panic!("building the fixture raw cache: {error:#}"))
}

/// A provenance equal to the pins: the dump's digest, the query commit and
/// all 114 file digests. The fixture queries are not JOB's files, so the
/// digests describe nothing about them; the point is that a cache with this
/// provenance is one `raw::ensure` accepts as current, whether `ensure` only
/// opens a finished cache or also compares its provenance with the pins.
pub fn pinned_provenance(duckdb: &Duckdb) -> RawProvenance {
    RawProvenance {
        imdb_sha256: pins::IMDB_SHA256.to_owned(),
        query_commit: pins::QUERY_COMMIT.to_owned(),
        query_sha256: pins::QUERY_FILES
            .iter()
            .map(|(file, digest)| ((*file).to_owned(), (*digest).to_owned()))
            .collect(),
        duckdb_version: duckdb.version().to_owned(),
    }
}
