//! The DuckDB command-line client that the JOB generator drives.
//!
//! Every call starts one `duckdb` process with the same session settings: a
//! [`MEMORY_LIMIT`] and a spill directory, so DuckDB's joins, sorts and
//! aggregates go to disk past the limit instead of claiming most of the
//! host's RAM beside a timing run, `preserve_insertion_order` so that an
//! `ORDER BY` result is not reshuffled on its way out, and no progress bar
//! (which the CLI may draw after about two seconds, into the row stream or
//! into stderr). The spill directory is private to the call: a fresh
//! subdirectory of the caller's `temp_dir`, removed when the call ends, because
//! DuckDB names its spill files by size alone and deletes every
//! `duckdb_temp_*` file in its directory on exit, so two concurrent
//! processes sharing one directory corrupt each other's spills (a run can
//! exit 0 with wrong rows). [`Duckdb::stream_rows`]
//! turns a `SELECT` into [`Tuples`] batches and never holds more than one.

use {
    anyhow::{anyhow, ensure, Context},
    kermit_ds::Tuples,
    std::{
        ffi::OsString,
        fs,
        io::{BufRead, BufReader, Read},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::atomic::{AtomicU64, Ordering},
        thread,
    },
};

/// The environment variable that names the DuckDB binary. When it is unset or
/// empty, `duckdb` is looked up on `PATH`.
pub const DUCKDB_BIN_ENV: &str = "KERMIT_DUCKDB_BIN";

/// DuckDB's `memory_limit` in every session. Past it, DuckDB's join, sort and
/// aggregate operators spill to the session's `temp_directory`.
pub const MEMORY_LIMIT: &str = "4GB";

/// Rows per [`Tuples`] batch that [`Duckdb::stream_rows`] hands to its sink
/// (the last batch of a stream is shorter).
pub const BATCH_ROWS: usize = 1 << 16;

/// `text` as a single-quoted SQL string literal (`'` doubled).
pub(crate) fn sql_literal(text: &str) -> String { format!("'{}'", text.replace('\'', "''")) }

/// `path` as a SQL string literal. DuckDB receives the statement as text, so
/// the path must be valid UTF-8.
pub(crate) fn path_literal(path: &Path) -> anyhow::Result<String> {
    let text = path
        .to_str()
        .with_context(|| format!("path {} is not valid UTF-8", path.display()))?;
    Ok(sql_literal(text))
}

/// One call's private spill directory, `<parent>/<process id>-<sequence>`,
/// removed (with whatever DuckDB left in it) when the guard drops. Hold it
/// until the child has been reaped.
#[derive(Debug)]
struct SpillDir {
    path: PathBuf,
}

impl SpillDir {
    /// The next directory name under `parent`: the process id keeps two
    /// processes sharing a `parent` apart, the process-wide sequence keeps two
    /// calls (or threads) of this process apart.
    fn next_path(parent: &Path) -> PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        parent.join(format!("{}-{sequence}", std::process::id()))
    }

    /// Creates a fresh directory (and `parent`, if missing).
    fn create(parent: &Path) -> anyhow::Result<SpillDir> {
        let path = Self::next_path(parent);
        fs::create_dir_all(&path)
            .with_context(|| format!("creating DuckDB's spill directory {}", path.display()))?;
        Ok(SpillDir {
            path,
        })
    }
}

impl Drop for SpillDir {
    fn drop(&mut self) {
        // Already gone is fine; any other failure leaves a stray directory
        // under the caller's `temp_dir`, which the next cache clean removes.
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A located DuckDB CLI.
#[derive(Debug, Clone)]
pub struct Duckdb {
    bin: PathBuf,
    version: String,
}

impl Duckdb {
    /// Finds the DuckDB CLI: the binary `$KERMIT_DUCKDB_BIN` names, else
    /// `duckdb` on `PATH`, and runs it with `--version` to be sure it works.
    ///
    /// # Errors
    ///
    /// Fails when the binary is missing or does not run; the message says to
    /// enter `nix develop` or to set `KERMIT_DUCKDB_BIN`.
    pub fn locate() -> anyhow::Result<Duckdb> {
        Self::locate_from(std::env::var_os(DUCKDB_BIN_ENV))
    }

    /// [`locate`](Self::locate) with the environment variable's value passed
    /// in, so a test can exercise the override without touching the process
    /// environment.
    fn locate_from(env: Option<OsString>) -> anyhow::Result<Duckdb> {
        match env.filter(|value| !value.is_empty()) {
            | Some(bin) => {
                let bin = PathBuf::from(bin);
                Self::at(bin.clone()).with_context(|| {
                    format!(
                        "`{DUCKDB_BIN_ENV}` is set to {}, which cannot be run",
                        bin.display()
                    )
                })
            },
            | None => Self::at(PathBuf::from("duckdb")).with_context(|| {
                format!(
                    "no usable `duckdb` on PATH: enter `nix develop` (the flake provides it) or \
                     set `{DUCKDB_BIN_ENV}` to a DuckDB CLI"
                )
            }),
        }
    }

    /// Uses the DuckDB CLI at `bin`, after running `bin --version`.
    ///
    /// # Errors
    ///
    /// Fails when `bin` cannot be run or `--version` exits non-zero or prints
    /// nothing.
    pub fn at(bin: PathBuf) -> anyhow::Result<Duckdb> {
        let output = Command::new(&bin)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running `{} --version`", bin.display()))?;
        ensure!(
            output.status.success(),
            "`{} --version` exited with {}",
            bin.display(),
            output.status
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout.lines().next().unwrap_or("").trim().to_owned();
        ensure!(
            !version.is_empty(),
            "`{} --version` printed nothing",
            bin.display()
        );
        Ok(Duckdb {
            bin,
            version,
        })
    }

    /// The first line of `duckdb --version`, trimmed (`v1.4.4 (Andium)
    /// 6ddac802ff`).
    pub fn version(&self) -> &str { &self.version }

    /// Runs `sql` against `database` read-write, creating the file if it is
    /// missing. This is the raw-cache build's call; everything else is
    /// read-only. DuckDB spills into a fresh subdirectory of `temp_dir`,
    /// removed when the call returns.
    ///
    /// # Errors
    ///
    /// Fails if DuckDB cannot start or exits non-zero; the message quotes its
    /// stderr.
    pub fn execute(&self, database: &Path, temp_dir: &Path, sql: &str) -> anyhow::Result<()> {
        let (command, _spill) = self.command(database, temp_dir, false, sql)?;
        self.capture(command, database)?;
        Ok(())
    }

    /// Runs a read-only query that returns one non-negative integer, such as a
    /// `COUNT(*)`. `temp_dir` is the parent of the call's spill directory.
    ///
    /// # Errors
    ///
    /// Fails if DuckDB fails, or if its output is not exactly one non-negative
    /// integer.
    pub fn query_u64(&self, database: &Path, temp_dir: &Path, sql: &str) -> anyhow::Result<u64> {
        let (command, _spill) = self.command(database, temp_dir, true, sql)?;
        let stdout = self.capture(command, database)?;
        let text = stdout.trim();
        text.parse::<u64>().with_context(|| {
            format!(
                "expected one non-negative integer from DuckDB on {}, got `{text}`",
                database.display()
            )
        })
    }

    /// Runs a read-only query with one column and returns one string per output
    /// line, exactly as DuckDB's CSV output prints it (no unquoting). For names
    /// and other plain text, such as `information_schema.tables`. `temp_dir` is
    /// the parent of the call's spill directory.
    ///
    /// # Errors
    ///
    /// Fails if DuckDB fails.
    pub fn query_strings(
        &self, database: &Path, temp_dir: &Path, sql: &str,
    ) -> anyhow::Result<Vec<String>> {
        let (command, _spill) = self.command(database, temp_dir, true, sql)?;
        let stdout = self.capture(command, database)?;
        Ok(stdout.lines().map(str::to_owned).collect())
    }

    /// Streams `COPY ({select}) TO '/dev/stdout' (FORMAT csv, HEADER false)`
    /// read-only, parses each output line into `arity` integers and calls
    /// `on_batch` with batches of [`BATCH_ROWS`] rows (the last one shorter;
    /// never for an empty result). Returns the row count.
    ///
    /// Memory is one batch here and at most [`MEMORY_LIMIT`] in DuckDB, which
    /// spills into a fresh subdirectory of `temp_dir`. The
    /// rows must be non-negative integers (cast to `BIGINT` in `select`); a
    /// NULL prints as an empty field and is an error naming its line.
    ///
    /// # Errors
    ///
    /// Succeeds only if DuckDB exits 0 with an empty stderr; otherwise the
    /// error quotes stderr. A line that does not parse is an error naming its
    /// line number. If `on_batch` fails, DuckDB is killed and that error is
    /// returned unchanged. Batches handed off before an error stay handed
    /// off, so a caller must not treat a partial stream as complete.
    pub fn stream_rows(
        &self, database: &Path, temp_dir: &Path, select: &str, arity: usize,
        on_batch: &mut dyn FnMut(&Tuples) -> anyhow::Result<()>,
    ) -> anyhow::Result<u64> {
        ensure!(arity > 0, "stream_rows needs at least one column");
        // The newline ends a trailing `--` comment in `select` before the `)`.
        let copy = format!("COPY ({select}\n) TO '/dev/stdout' (FORMAT csv, HEADER false)");
        // `_spill` outlives the child: every path below reaps it before this
        // function returns, and locals drop after the body's last statement.
        let (mut command, _spill) = self.command(database, temp_dir, true, &copy)?;
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", self.bin.display()))?;
        let mut stderr = child
            .stderr
            .take()
            .context("DuckDB's stderr was not piped")?;
        let stderr_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            // A read error leaves whatever arrived; the exit status still
            // decides. Bytes, not `read_to_string`: stderr that is
            // not UTF-8 must not lose the rest of the message.
            let _ = stderr.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        });
        let stdout = child
            .stdout
            .take()
            .context("DuckDB's stdout was not piped")?;
        match read_batches(BufReader::with_capacity(1 << 20, stdout), arity, on_batch) {
            | Ok(rows) => {
                let status = child.wait().context("waiting for DuckDB")?;
                let stderr = stderr_reader.join().unwrap_or_default();
                ensure!(
                    status.success() && stderr.trim().is_empty(),
                    "DuckDB failed ({status}) streaming from {}:\n{}",
                    database.display(),
                    stderr.trim()
                );
                Ok(rows)
            },
            | Err(failure) => {
                // Stop DuckDB before it blocks on a pipe nobody reads.
                let _ = child.kill();
                let _ = child.wait();
                let stderr = stderr_reader.join().unwrap_or_default();
                match failure {
                    | StreamFailure::Sink(error) => Err(error),
                    | StreamFailure::Input(error) if stderr.trim().is_empty() => Err(error),
                    | StreamFailure::Input(error) => {
                        Err(error.context(format!("DuckDB reported:\n{}", stderr.trim())))
                    },
                }
            },
        }
    }

    /// The command that runs `sql` against `database` under the session
    /// settings: `duckdb [-readonly] <database> -csv -noheader -c "<settings>
    /// <sql>"`, and the spill directory it uses.
    ///
    /// That directory is `<temp_dir>/<process id>-<sequence>`, created here and
    /// private to this call. DuckDB would otherwise share `temp_dir` between
    /// concurrent processes, which write identically named spill files there
    /// and each delete all of them on exit. The caller keeps the returned
    /// [`SpillDir`] until the child has been reaped; dropping it removes the
    /// directory, on success and on every error path.
    fn command(
        &self, database: &Path, temp_dir: &Path, read_only: bool, sql: &str,
    ) -> anyhow::Result<(Command, SpillDir)> {
        let spill = SpillDir::create(temp_dir)?;
        let script = format!(
            "SET memory_limit='{MEMORY_LIMIT}'; SET temp_directory={}; SET \
             preserve_insertion_order=true; SET enable_progress_bar=false; {sql}",
            path_literal(&spill.path)?
        );
        let mut command = Command::new(&self.bin);
        if read_only {
            command.arg("-readonly");
        }
        command
            .arg(database)
            .args(["-csv", "-noheader", "-c"])
            .arg(script)
            .stdin(Stdio::null());
        Ok((command, spill))
    }

    /// Runs `command` to completion and returns its stdout, or an error that
    /// quotes its stderr when it exits non-zero.
    fn capture(&self, mut command: Command, database: &Path) -> anyhow::Result<String> {
        let output = command
            .output()
            .with_context(|| format!("running {}", self.bin.display()))?;
        ensure!(
            output.status.success(),
            "DuckDB failed ({}) on {}:\n{}",
            output.status,
            database.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Why `read_batches` stopped: the sink refused a batch, or the input was
/// not what `stream_rows` expects.
#[derive(Debug)]
enum StreamFailure {
    /// `on_batch` returned this error.
    Sink(anyhow::Error),
    /// DuckDB's output could not be read or parsed.
    Input(anyhow::Error),
}

/// Parses `lines` into `arity`-integer rows and hands them to `on_batch` in
/// batches of [`BATCH_ROWS`]; returns the row count. A fresh batch is
/// allocated after each hand-off: [`Tuples`] has no `clear()`, so a new batch
/// of the same arity and capacity is how the next one starts.
fn read_batches(
    mut lines: impl BufRead, arity: usize, on_batch: &mut dyn FnMut(&Tuples) -> anyhow::Result<()>,
) -> Result<u64, StreamFailure> {
    let mut batch = Tuples::with_capacity(arity, BATCH_ROWS);
    let mut row = Vec::with_capacity(arity);
    let mut line = String::new();
    let mut total = 0u64;
    loop {
        line.clear();
        let read = lines.read_line(&mut line).map_err(|error| {
            StreamFailure::Input(anyhow!(error).context("reading DuckDB's output"))
        })?;
        if read == 0 {
            break;
        }
        total += 1;
        parse_row(line.trim_end_matches(['\n', '\r']), arity, total, &mut row)
            .map_err(StreamFailure::Input)?;
        batch.push(&row);
        if batch.len() == BATCH_ROWS {
            on_batch(&batch).map_err(StreamFailure::Sink)?;
            batch = Tuples::with_capacity(arity, BATCH_ROWS);
        }
    }
    if !batch.is_empty() {
        on_batch(&batch).map_err(StreamFailure::Sink)?;
    }
    Ok(total)
}

/// Parses one output line (`text`, number `line_number`) into `row`.
fn parse_row(
    text: &str, arity: usize, line_number: u64, row: &mut Vec<usize>,
) -> anyhow::Result<()> {
    row.clear();
    for field in text.split(',') {
        let value = field.parse::<usize>().map_err(|_| {
            anyhow!(
                "DuckDB output line {line_number}: `{text}` is not {arity} comma-separated \
                 non-negative integers (a NULL prints as an empty field)"
            )
        })?;
        row.push(value);
    }
    ensure!(
        row.len() == arity,
        "DuckDB output line {line_number}: `{text}` has {} fields, expected {arity}",
        row.len()
    );
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::Duckdb;

    /// The located DuckDB, or `None` after a stderr line saying why: the
    /// DuckDB-gated tests skip, as the LUBM tests skip without `java`.
    pub(crate) fn locate_or_skip() -> Option<Duckdb> {
        match Duckdb::locate() {
            | Ok(duckdb) => Some(duckdb),
            | Err(error) => {
                eprintln!("skipping: no DuckDB ({error:#})");
                None
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{test_support::locate_or_skip, *},
        std::{io::Cursor, os::unix::fs::PermissionsExt},
    };

    /// A scratch database holding one empty table `t`, plus its spill
    /// directory.
    struct Scratch {
        _dir: tempfile::TempDir,
        database: PathBuf,
        temp: PathBuf,
    }

    impl Scratch {
        fn new(duckdb: &Duckdb) -> Scratch {
            let dir = tempfile::tempdir().unwrap();
            let database = dir.path().join("scratch.duckdb");
            let temp = dir.path().join("tmp");
            duckdb
                .execute(&database, &temp, "CREATE TABLE t (a INTEGER)")
                .unwrap();
            Scratch {
                _dir: dir,
                database,
                temp,
            }
        }
    }

    /// A `#!/bin/sh` stand-in for the DuckDB CLI in `dir`: `--version` prints a
    /// version line and exits 0; any other invocation prints `stderr` (first,
    /// so it has arrived by the time a row is read), then `stdout`, then
    /// exits `code`.
    fn fake_duckdb(dir: &Path, stdout: &str, stderr: &str, code: i32) -> Duckdb {
        let bin = dir.join("fake-duckdb");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'v0.0.0 (fake)'; exit 0; fi\ncat \
             >&2 <<'ERR'\n{stderr}\nERR\ncat <<'OUT'\n{stdout}\nOUT\nexit {code}\n"
        );
        fs::write(&bin, script).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        // Another test thread may fork while the script's write handle is
        // open, and the child holds that handle until its exec: executing the
        // script then fails once with ETXTBSY, so retry that error only.
        for _ in 0..50 {
            match Duckdb::at(bin.clone()) {
                | Err(error) if format!("{error:#}").contains("Text file busy") => {
                    thread::sleep(std::time::Duration::from_millis(20));
                },
                | result => return result.unwrap(),
            }
        }
        panic!("{} stayed busy", bin.display())
    }

    /// The number of entries directly under `dir`.
    fn entries(dir: &Path) -> usize { fs::read_dir(dir).unwrap().count() }

    // ---- no DuckDB needed -------------------------------------------------

    #[test]
    fn sql_literal_doubles_quotes() {
        assert_eq!(sql_literal("plain"), "'plain'");
        assert_eq!(sql_literal("it's"), "'it''s'");
        assert_eq!(sql_literal(""), "''");
    }

    /// The env override names a binary that does not exist: the error says
    /// which variable and which path, so a mistyped override is not mistaken
    /// for a missing install.
    #[test]
    fn locate_honours_the_env_override() {
        let missing = "/nonexistent/kermit-test/duckdb";
        let error = Duckdb::locate_from(Some(OsString::from(missing))).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(missing), "{message}");
        assert!(message.contains(DUCKDB_BIN_ENV), "{message}");
    }

    #[test]
    fn read_batches_cuts_at_batch_rows() {
        let text: String = (0..70_000).map(|i| format!("{i},{}\n", i * 2)).collect();
        let mut sizes = Vec::new();
        let mut next = 0usize;
        let rows = read_batches(Cursor::new(text), 2, &mut |batch: &Tuples| {
            sizes.push(batch.len());
            for row in batch.rows() {
                assert_eq!(row, &[next, next * 2][..]);
                next += 1;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(sizes, [BATCH_ROWS, 70_000 - BATCH_ROWS]);
        assert_eq!((rows, next), (70_000, 70_000));
    }

    #[test]
    fn read_batches_empty_input_calls_nothing() {
        let mut calls = 0;
        let rows = read_batches(Cursor::new(""), 3, &mut |_: &Tuples| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!((rows, calls), (0, 0));
    }

    /// A line that is not `arity` integers is an input failure naming its line
    /// number, and rows before it in an unfinished batch are never handed off.
    #[test]
    fn read_batches_names_the_bad_line() {
        for (text, arity) in [
            ("1,2\n3,x\n", 2),
            ("1,2\n3\n", 2),
            ("1\n\n", 1),
            ("1,2,3\n", 2),
        ] {
            let mut calls = 0;
            let failure = read_batches(Cursor::new(text), arity, &mut |_: &Tuples| {
                calls += 1;
                Ok(())
            })
            .unwrap_err();
            assert!(
                matches!(&failure, StreamFailure::Input(error)
                    if error.to_string().contains("line ")),
                "{text:?}: {failure:?}"
            );
            assert_eq!(calls, 0, "{text:?}");
        }
        let failure =
            read_batches(Cursor::new("1,2\n3,x\n"), 2, &mut |_: &Tuples| Ok(())).unwrap_err();
        assert!(
            matches!(&failure, StreamFailure::Input(error) if error.to_string().contains("line 2")),
            "{failure:?}"
        );
    }

    /// The sink's own error comes back as the sink's, unwrapped, so the caller
    /// can tell a refused batch from malformed input.
    #[test]
    fn read_batches_reports_a_sink_error_as_the_sinks() {
        let failure = read_batches(Cursor::new("1\n2\n"), 1, &mut |_: &Tuples| {
            Err(anyhow!("sink refused"))
        })
        .unwrap_err();
        assert!(
            matches!(&failure, StreamFailure::Sink(error) if error.to_string() == "sink refused"),
            "{failure:?}"
        );
    }

    /// Spill directories are distinct per call, live directly under the
    /// caller's `temp_dir` (created if missing) and are named by process id,
    /// and dropping one removes it and nothing else; a directory that is
    /// already gone drops quietly.
    #[test]
    fn spill_dirs_are_distinct_and_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("duckdb.tmp");
        let first = SpillDir::create(&parent).unwrap();
        let second = SpillDir::create(&parent).unwrap();
        assert_ne!(first.path, second.path);
        for spill in [&first, &second] {
            assert_eq!(spill.path.parent(), Some(parent.as_path()));
            assert!(spill.path.is_dir());
            let name = spill.path.file_name().unwrap().to_str().unwrap();
            assert!(
                name.starts_with(&format!("{}-", std::process::id())),
                "{name}"
            );
        }
        let kept = second.path.clone();
        drop(first);
        assert_eq!(entries(&parent), 1);
        assert!(kept.is_dir());
        fs::remove_dir_all(&kept).unwrap();
        drop(second);
        assert_eq!(entries(&parent), 0);
    }

    /// `command()` points DuckDB's `temp_directory` at the call's own
    /// subdirectory, never at `temp_dir` itself, and two commands get two
    /// different ones. (This is the wiring that keeps concurrent DuckDB
    /// processes from sharing spill files.)
    #[test]
    fn command_spills_into_its_own_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let duckdb = fake_duckdb(dir.path(), "", "", 0);
        let temp = dir.path().join("duckdb.tmp");
        let mut spills = Vec::new();
        for _ in 0..2 {
            let (command, spill) = duckdb
                .command(&dir.path().join("db"), &temp, true, "SELECT 1")
                .unwrap();
            let script = command
                .get_args()
                .last()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            let wanted = format!("SET temp_directory={};", path_literal(&spill.path).unwrap());
            assert!(script.contains(&wanted), "{script}");
            assert_eq!(spill.path.parent(), Some(temp.as_path()));
            spills.push(spill);
        }
        assert_ne!(spills[0].path, spills[1].path);
        drop(spills);
        assert_eq!(entries(&temp), 0);
    }

    /// Exit 0 with anything on stderr is a failure, and the error quotes
    /// stderr, though the rows had already been handed off.
    #[test]
    fn stream_rows_fails_on_stderr_despite_exit_zero() {
        let dir = tempfile::tempdir().unwrap();
        let duckdb = fake_duckdb(dir.path(), "1\n2", "WARNING: something odd", 0);
        let temp = dir.path().join("duckdb.tmp");
        let mut calls = 0;
        let error = duckdb
            .stream_rows(&dir.path().join("db"), &temp, "SELECT a", 1, &mut |_| {
                calls += 1;
                Ok(())
            })
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("DuckDB failed (exit status: 0)"),
            "{message}"
        );
        assert!(message.contains("WARNING: something odd"), "{message}");
        assert_eq!(calls, 1);
        assert_eq!(entries(&temp), 0, "the spill directory is removed");
    }

    /// Rows followed by a non-zero exit is a failure quoting stderr and the
    /// status: a crash after output is not a short result.
    #[test]
    fn stream_rows_fails_on_a_nonzero_exit_after_rows() {
        let dir = tempfile::tempdir().unwrap();
        let duckdb = fake_duckdb(dir.path(), "1\n2", "Out of Memory Error: boom", 3);
        let temp = dir.path().join("duckdb.tmp");
        let mut seen = Vec::new();
        let error = duckdb
            .stream_rows(
                &dir.path().join("db"),
                &temp,
                "SELECT a",
                1,
                &mut |batch: &Tuples| {
                    seen.extend(batch.rows().map(|row| row[0]));
                    Ok(())
                },
            )
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("DuckDB failed (exit status: 3)"),
            "{message}"
        );
        assert!(message.contains("Out of Memory Error: boom"), "{message}");
        assert_eq!(seen, [1, 2]);
        assert_eq!(entries(&temp), 0, "the spill directory is removed");
    }

    /// A malformed row is an error naming its line, with DuckDB's stderr
    /// attached when it printed any, and without it when it printed none.
    #[test]
    fn stream_rows_malformed_row_carries_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("duckdb.tmp");
        let duckdb = fake_duckdb(dir.path(), "1\nx", "Conversion Error: bad value", 0);
        let mut calls = 0;
        let error = duckdb
            .stream_rows(&dir.path().join("db"), &temp, "SELECT a", 1, &mut |_| {
                calls += 1;
                Ok(())
            })
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("line 2"), "{message}");
        assert!(message.contains("DuckDB reported:"), "{message}");
        assert!(message.contains("Conversion Error: bad value"), "{message}");
        assert_eq!(calls, 0);
        assert_eq!(entries(&temp), 0, "the spill directory is removed");

        let duckdb = fake_duckdb(dir.path(), "1\nx", "", 0);
        let error = duckdb
            .stream_rows(
                &dir.path().join("db"),
                &temp,
                "SELECT a",
                1,
                &mut |_| Ok(()),
            )
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("line 2"), "{message}");
        assert!(!message.contains("DuckDB reported:"), "{message}");
    }

    // ---- DuckDB-gated ----------------------------------------------------

    /// No call leaves anything under its `temp_dir`: not after a success, not
    /// after a failed statement, not after a sink that stops the stream.
    #[test]
    fn calls_leave_no_spill_directory_behind() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        assert_eq!(entries(&scratch.temp), 0, "after execute");
        duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT COUNT(*) FROM t")
            .unwrap();
        duckdb
            .query_strings(&scratch.database, &scratch.temp, "SELECT 'x'")
            .unwrap();
        duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT range AS a FROM range(3)",
                1,
                &mut |_| Ok(()),
            )
            .unwrap();
        assert_eq!(entries(&scratch.temp), 0, "after the successes");
        assert!(duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT * FROM nope")
            .is_err());
        assert!(duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT range AS a FROM range(5000000) ORDER BY a",
                1,
                &mut |_| Err(anyhow!("sink refused")),
            )
            .is_err());
        assert_eq!(entries(&scratch.temp), 0, "after the failures");
    }

    /// 70,000 rows arrive as one full batch and one remainder, in `ORDER BY`
    /// order, and the call returns the row count.
    #[test]
    fn stream_rows_batches_and_orders() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        let mut sizes = Vec::new();
        let mut next = 0usize;
        let rows = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT range AS a, range * 2 AS b FROM range(70000) ORDER BY a",
                2,
                &mut |batch: &Tuples| {
                    sizes.push(batch.len());
                    for row in batch.rows() {
                        assert_eq!(row, &[next, next * 2][..]);
                        next += 1;
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(sizes, [65_536, 4_464]);
        assert_eq!((rows, next), (70_000, 70_000));
    }

    /// An empty result never calls the sink: the row count would otherwise
    /// include a stray line (DuckDB's own `Count` row would show up here).
    #[test]
    fn stream_rows_empty_result_calls_nothing() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        let mut calls = 0;
        let rows = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT a FROM t",
                1,
                &mut |_| {
                    calls += 1;
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!((rows, calls), (0, 0));
    }

    /// One column is the shape a unary relation has, and the one where a
    /// trailing count line could not be told from a row.
    #[test]
    fn stream_rows_one_column_reads_exactly_the_rows() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        let mut seen = Vec::new();
        let rows = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT range AS a FROM range(5) ORDER BY a",
                1,
                &mut |batch: &Tuples| {
                    seen.extend(batch.rows().map(|row| row[0]));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(seen, [0, 1, 2, 3, 4]);
        assert_eq!(rows, 5);
    }

    /// A sink that fails stops the stream at its first batch: the error is the
    /// sink's, unwrapped, and the sink is called once. (`stream_rows` kills
    /// DuckDB instead of waiting for the 5,000,000 rows; the test does not
    /// time that.)
    #[test]
    fn stream_rows_stops_when_the_sink_fails() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        let mut calls = 0;
        let error = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT range AS a FROM range(5000000) ORDER BY a",
                1,
                &mut |_| {
                    calls += 1;
                    Err(anyhow!("sink refused"))
                },
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "sink refused");
        assert_eq!(calls, 1);
    }

    /// A failing statement is an error that quotes DuckDB's stderr, from every
    /// entry point, and a stream that failed never reached its sink.
    #[test]
    fn sql_error_quotes_stderr() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        let error = duckdb
            .query_u64(
                &scratch.database,
                &scratch.temp,
                "SELECT COUNT(*) FROM no_such_table",
            )
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("Catalog Error"), "{message}");
        assert!(message.contains("no_such_table"), "{message}");

        let mut calls = 0;
        let error = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT a FROM no_such_table",
                1,
                &mut |_| {
                    calls += 1;
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("Catalog Error"), "{error:#}");
        assert_eq!(calls, 0);
    }

    /// `query_u64` opens the database read-only: a write is refused and the
    /// table is unchanged.
    #[test]
    fn readonly_refuses_writes() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        let refused =
            duckdb.query_u64(&scratch.database, &scratch.temp, "INSERT INTO t VALUES (1)");
        assert!(refused.is_err());
        let rows = duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT COUNT(*) FROM t")
            .unwrap();
        assert_eq!(rows, 0);
    }

    /// Every read entry point (`query_u64`, `query_strings`, `stream_rows`)
    /// opens the database read-only: with the file itself made read-only they
    /// still succeed, which a read-write open could not. Skipped as root, for
    /// whom the file stays writable.
    #[test]
    fn read_entry_points_work_on_a_read_only_file() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        duckdb
            .execute(
                &scratch.database,
                &scratch.temp,
                "INSERT INTO t SELECT range FROM range(4)",
            )
            .unwrap();
        fs::set_permissions(&scratch.database, fs::Permissions::from_mode(0o444)).unwrap();
        if fs::OpenOptions::new()
            .write(true)
            .open(&scratch.database)
            .is_ok()
        {
            eprintln!("skipping: the file is still writable (running as root?)");
            return;
        }
        let rows = duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT COUNT(*) FROM t")
            .unwrap();
        assert_eq!(rows, 4);
        let names = duckdb
            .query_strings(
                &scratch.database,
                &scratch.temp,
                "SELECT table_name FROM information_schema.tables WHERE table_schema = 'main'",
            )
            .unwrap();
        assert_eq!(names, ["t"]);
        let mut seen = 0;
        let streamed = duckdb
            .stream_rows(
                &scratch.database,
                &scratch.temp,
                "SELECT a FROM t",
                1,
                &mut |batch: &Tuples| {
                    seen += batch.len();
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!((streamed, seen), (4, 4));
    }

    #[test]
    fn query_u64_reads_a_count() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        duckdb
            .execute(
                &scratch.database,
                &scratch.temp,
                "INSERT INTO t SELECT range FROM range(7)",
            )
            .unwrap();
        let rows = duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT COUNT(*) FROM t")
            .unwrap();
        assert_eq!(rows, 7);
        // Text, or a negative number, is an error and not a zero.
        let text = duckdb.query_u64(&scratch.database, &scratch.temp, "SELECT 'seven'");
        assert!(format!("{:#}", text.unwrap_err()).contains("seven"));
        assert!(duckdb
            .query_u64(&scratch.database, &scratch.temp, "SELECT -1")
            .is_err());
    }

    #[test]
    fn query_strings_reads_a_text_column() {
        let Some(duckdb) = locate_or_skip() else {
            return;
        };
        let scratch = Scratch::new(&duckdb);
        duckdb
            .execute(
                &scratch.database,
                &scratch.temp,
                "CREATE TABLE u (b INTEGER)",
            )
            .unwrap();
        let names = duckdb
            .query_strings(
                &scratch.database,
                &scratch.temp,
                "SELECT table_name FROM information_schema.tables WHERE table_schema = 'main' AND \
                 table_type = 'BASE TABLE' ORDER BY table_name",
            )
            .unwrap();
        assert_eq!(names, ["t", "u"]);
    }
}
