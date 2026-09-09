# Sweep Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `bench run` sweeps crash-safe, make the binary find its workspace at runtime, and pin the identity strings that name Criterion directories and report axes.

**Architecture:** Three independent changes from `docs/specs/2026-09-09-sweep-hardening-design.md`. (1) `axis_value()` on `IndexStructure` / `JoinAlgorithm` replaces `format!("{:?}")` at the three label sites in `kermit/src/main.rs`. (2) `materialize::workspace_root` resolves from `KERMIT_WORKSPACE`, then a walk-up from the current directory, then the compile-time path; the resolution is a pure function so it is unit-testable without touching process state. (3) A `ReportSink` in `kermit/src/bench_report.rs` rewrites the JSON report atomically after every cell; the three `bench` handlers go through it.

**Tech Stack:** Rust nightly (see `rust-toolchain.toml`), clap, serde_json, tempfile (dev). Run every `cargo fmt` inside `nix develop` — stable rustfmt rewrites the tree. Run builds in the foreground with `CARGO_BUILD_JOBS=2` (see the memory note on the background-job memory monitor).

**Conventions to follow:** `rustfmt.toml` uses `match_arm_leading_pipes = "Always"` (every match arm starts with `| `), `imports_granularity = "One"` (a single `use { ... };` block per file), `max_width = 100`. Doc comments are linted by clippy `doc_markdown` under `-Dwarnings`: backtick every identifier. Commit messages are conventional commits and end with the attribution trailer given in the session.

---

## File map

| File | Change |
| --- | --- |
| `kermit-ds/src/ds/mod.rs` | add `IndexStructure::axis_value` + tests |
| `kermit-algos/src/lib.rs` | add `JoinAlgorithm::axis_value` + tests |
| `kermit/src/main.rs` | use `axis_value()` at three sites; replace `write_bench_report` / `default_report_path` with `ReportSink` in three handlers |
| `docs/specs/bench-report-schema.md` | rows for `data_structure` / `algorithm` |
| `kermit/src/materialize.rs` | runtime `workspace_root` + pure `resolve_workspace_root` + tests |
| `kermit/src/bench_report.rs` | `ReportSink` + tests |
| `kermit/tests/cli_bench_run_sweep.rs` | partial-sweep CLI test |

---

### Task 1: `IndexStructure::axis_value`

**Files:**
- Modify: `kermit-ds/src/ds/mod.rs` (after the `impl FromStr for IndexStructure` block, around line 45)

- [ ] **Step 1: Write the failing tests**

Append to the bottom of `kermit-ds/src/ds/mod.rs`:

```rust
#[cfg(test)]
mod index_structure_tests {
    use {super::*, clap::ValueEnum};

    /// Pins every label to a literal. These strings name
    /// `target/criterion/{group}` directories and the report's
    /// `data_structure` axis, so a change here repartitions every
    /// measurement taken so far. Change deliberately or not at all.
    #[test]
    fn axis_values_are_pinned() {
        assert_eq!(IndexStructure::ColumnTrie.axis_value(), "ColumnTrie");
        assert_eq!(IndexStructure::HashTrie.axis_value(), "HashTrie");
        assert_eq!(IndexStructure::TreeTrie.axis_value(), "TreeTrie");
    }

    /// The label used to be `format!("{:?}")`. Keeping the two equal means
    /// historical Criterion directories keep their names; if you ever
    /// diverge them, update this test *and* decide what happens to old
    /// measurements.
    #[test]
    fn axis_values_match_debug_repr() {
        for v in IndexStructure::value_variants() {
            assert_eq!(v.axis_value(), format!("{v:?}"));
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds index_structure_tests`
Expected: compile error `no method named `axis_value` found for enum `IndexStructure``.

- [ ] **Step 3: Implement**

Insert directly after the `impl FromStr for IndexStructure { ... }` block in `kermit-ds/src/ds/mod.rs`:

```rust
impl IndexStructure {
    /// The bench-report axis value for this structure (the
    /// `data_structure` key) and the label embedded in Criterion group
    /// names. This is a stable external contract: it names on-disk
    /// `target/criterion/` directories, so existing measurements are
    /// repartitioned if it changes. Equal to the `Debug` representation
    /// for historical continuity.
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::ColumnTrie => "ColumnTrie",
            | Self::HashTrie => "HashTrie",
            | Self::TreeTrie => "TreeTrie",
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-ds index_structure_tests`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Commit**

```bash
git add kermit-ds/src/ds/mod.rs
git commit -m "feat(kermit-ds): add IndexStructure::axis_value as the pinned identity label"
```

---

### Task 2: `JoinAlgorithm::axis_value`

**Files:**
- Modify: `kermit-algos/src/lib.rs` (after the `impl FromStr for JoinAlgorithm` block, around line 70)

- [ ] **Step 1: Write the failing tests**

Insert directly before the existing `#[cfg(test)] mod optimiser_enum_tests` in `kermit-algos/src/lib.rs`:

```rust
#[cfg(test)]
mod join_algorithm_tests {
    use {super::*, clap::ValueEnum};

    /// Pins every label to a literal. These strings name
    /// `target/criterion/{group}` directories and the report's `algorithm`
    /// axis, so a change here repartitions every measurement taken so far.
    #[test]
    fn axis_values_are_pinned() {
        assert_eq!(JoinAlgorithm::HashTriejoin.axis_value(), "HashTriejoin");
        assert_eq!(JoinAlgorithm::LeapfrogTriejoin.axis_value(), "LeapfrogTriejoin");
    }

    /// The label used to be `format!("{:?}")`; keeping them equal preserves
    /// historical Criterion directory names. Diverge only with a deliberate
    /// decision about old measurements.
    #[test]
    fn axis_values_match_debug_repr() {
        for v in JoinAlgorithm::value_variants() {
            assert_eq!(v.axis_value(), format!("{v:?}"));
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-algos join_algorithm_tests`
Expected: compile error `no method named `axis_value` found for enum `JoinAlgorithm``.

- [ ] **Step 3: Implement**

Insert directly after the `impl FromStr for JoinAlgorithm { ... }` block:

```rust
impl JoinAlgorithm {
    /// The bench-report axis value for this algorithm (the `algorithm`
    /// key) and the label embedded in Criterion group names. This is a
    /// stable external contract: it names on-disk `target/criterion/`
    /// directories, so existing measurements are repartitioned if it
    /// changes. Equal to the `Debug` representation for historical
    /// continuity.
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::HashTriejoin => "HashTriejoin",
            | Self::LeapfrogTriejoin => "LeapfrogTriejoin",
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-algos join_algorithm_tests`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Commit**

```bash
git add kermit-algos/src/lib.rs
git commit -m "feat(kermit-algos): add JoinAlgorithm::axis_value as the pinned identity label"
```

---

### Task 3: Use `axis_value()` at the three label sites

**Files:**
- Modify: `kermit/src/main.rs` — `run_ds_bench` (~line 694), `run_benchmark` (~lines 876-883), `run_bench_join` (~lines 1246-1271)
- Modify: `docs/specs/bench-report-schema.md` lines 72-73

There are no new tests: the existing CLI tests (`cli_bench_run_sweep.rs`, `cli_end_to_end_metric.rs`) assert the literal strings `"TreeTrie"`, `"HashTrie"`, `"LeapfrogTriejoin"`, `"HashTriejoin"` in report axes and will fail if the labels change.

- [ ] **Step 1: `run_ds_bench`**

Replace

```rust
    // Stable external contract, not debug output: this string becomes the
    // report's `data_structure` axis and the `{ds_name}/<metric>` Criterion
    // function ids under `target/criterion/{group}/`. It is derived from
    // the family's own `Execution`, the value that picked the code path.
    let ds_name = format!("{:?}", family.execution().index_structure());
```

with

```rust
    // This string becomes the report's `data_structure` axis and the
    // `{ds_name}/<metric>` Criterion function ids under
    // `target/criterion/{group}/`. It is derived from the family's own
    // `Execution`, the value that picked the code path; the label itself is
    // pinned by `IndexStructure::axis_value`.
    let ds_name = family.execution().index_structure().axis_value();
```

Then, because `ds_name` is now `&'static str` rather than `String`, change the one place it is used by reference: `MetadataLine::new("data structure", &ds_name)` becomes `MetadataLine::new("data structure", ds_name)`. The `format!("{ds_name}/insertion")` and `serde_json::json!(ds_name)` uses compile unchanged.

- [ ] **Step 2: `run_benchmark`**

Replace

```rust
    // `ds_name`/`algo_name` are `Debug`-derived strings for the DS and
    // algorithm enums. These are a STABLE external contract, not throwaway
    // debug output: they become the report's identity axes and the on-disk
    // `target/criterion/{group}` names (the group_name below embeds them).
    // Changing the `Debug` output would silently repartition prior
    // benchmark measurements. See the same pattern in `run_ds_bench` and
    // the `bench join` arm.
    let execution = family.execution();
    let ds_name = format!("{:?}", execution.index_structure());
    let algo_name = format!("{:?}", execution.algorithm());
```

with

```rust
    // `ds_name`/`algo_name` become the report's identity axes and the
    // on-disk `target/criterion/{group}` names (the group_name below embeds
    // them). The labels are pinned by `IndexStructure::axis_value` and
    // `JoinAlgorithm::axis_value`; see the tests there before changing one.
    let execution = family.execution();
    let ds_name = execution.index_structure().axis_value();
    let algo_name = execution.algorithm().axis_value();
```

and change `MetadataLine::new("data structure", &ds_name)` / `MetadataLine::new("algorithm", &algo_name)` to pass `ds_name` / `algo_name` without `&`.

- [ ] **Step 3: `run_bench_join`**

Replace

```rust
    let bench_id = format!("{:?}/{:?}", query_args.indexstructure, query_args.algorithm);

    let metadata = vec![
        MetadataLine::new("data structure", format!("{:?}", query_args.indexstructure)),
        MetadataLine::new("algorithm", format!("{:?}", query_args.algorithm)),
        MetadataLine::new("relations", query_args.relations.len()),
    ];
```

with

```rust
    let ds_name = query_args.indexstructure.axis_value();
    let algo_name = query_args.algorithm.axis_value();
    let bench_id = format!("{ds_name}/{algo_name}");

    let metadata = vec![
        MetadataLine::new("data structure", ds_name),
        MetadataLine::new("algorithm", algo_name),
        MetadataLine::new("relations", query_args.relations.len()),
    ];
```

and in the `axes` map below it replace

```rust
        (
            "data_structure".to_string(),
            serde_json::json!(format!("{:?}", query_args.indexstructure)),
        ),
        (
            "algorithm".to_string(),
            serde_json::json!(format!("{:?}", query_args.algorithm)),
        ),
```

with

```rust
        ("data_structure".to_string(), serde_json::json!(ds_name)),
        ("algorithm".to_string(), serde_json::json!(algo_name)),
```

- [ ] **Step 4: Schema doc**

In `docs/specs/bench-report-schema.md` change the two rows:

```
| `data_structure` | `join`, `ds`, `run`      | string           | `"TreeTrie"`, `"ColumnTrie"`, `"HashTrie"`. The `IndexStructure::axis_value` string. |
| `algorithm`      | `join`, `run`            | string           | `"LeapfrogTriejoin"`, `"HashTriejoin"`. The `JoinAlgorithm::axis_value` string. |
```

- [ ] **Step 5: Build and run the CLI tests that pin the labels**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_run_sweep --test cli_end_to_end_metric`
Expected: all pass (the sweep tests skip on non-Linux with a stderr note; that is fine).

- [ ] **Step 6: Commit**

```bash
git add kermit/src/main.rs docs/specs/bench-report-schema.md
git commit -m "refactor(kermit): derive identity labels from axis_value instead of Debug"
```

---

### Task 4: Runtime workspace-root resolution

**Files:**
- Modify: `kermit/src/materialize.rs` lines 63-69 (`workspace_root`) and the `mod tests` block at the bottom

Design note: the spec suggested serialising tests on process state with a mutex. This plan instead splits resolution into a pure function `resolve_workspace_root(env_override, cwd, fallback)` that takes its inputs as arguments, so the tests never touch the real environment or current directory. It satisfies the same requirements with no global state.

- [ ] **Step 1: Write the failing tests**

Add inside the existing `#[cfg(test)] mod tests { ... }` in `kermit/src/materialize.rs` (anywhere after the `use` block):

```rust
    fn workspace_tree() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\"]\n",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        // A member crate manifest with no [workspace] table must be skipped.
        fs::write(
            tmp.path().join("a/Cargo.toml"),
            "[package]\nname = \"a\"\n",
        )
        .unwrap();
        tmp
    }

    #[test]
    fn workspace_root_walks_up_to_the_workspace_manifest() {
        let tmp = workspace_tree();
        let fallback = Path::new("/fallback");
        let (got, source) = resolve_workspace_root(None, &tmp.path().join("a/b"), fallback);
        assert_eq!(got, tmp.path());
        assert_eq!(source, RootSource::WalkUp);
    }

    #[test]
    fn workspace_root_env_override_wins() {
        let tmp = workspace_tree();
        let fallback = Path::new("/fallback");
        let (got, source) = resolve_workspace_root(
            Some("/explicit/workspace"),
            &tmp.path().join("a/b"),
            fallback,
        );
        assert_eq!(got, Path::new("/explicit/workspace"));
        assert_eq!(source, RootSource::Env);
    }

    #[test]
    fn workspace_root_empty_env_override_is_ignored() {
        let tmp = workspace_tree();
        let fallback = Path::new("/fallback");
        let (got, source) = resolve_workspace_root(Some(""), &tmp.path().join("a/b"), fallback);
        assert_eq!(got, tmp.path());
        assert_eq!(source, RootSource::WalkUp);
    }

    #[test]
    fn workspace_root_falls_back_when_no_manifest_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("x/y")).unwrap();
        let fallback = Path::new("/fallback");
        let (got, source) = resolve_workspace_root(None, &tmp.path().join("x/y"), fallback);
        assert_eq!(got, fallback);
        assert_eq!(source, RootSource::CompileTime);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit materialize::tests::workspace_root`
Expected: compile error `cannot find function `resolve_workspace_root``.

- [ ] **Step 3: Implement**

Replace the existing `workspace_root` function (and its doc comment) with:

```rust
/// Environment variable that pins the workspace root, bypassing the
/// walk-up. Useful for tests and for running an installed binary against a
/// checkout that is not the current directory.
pub(crate) const WORKSPACE_ENV: &str = "KERMIT_WORKSPACE";

/// Resolves the workspace root at runtime.
///
/// Order: the [`WORKSPACE_ENV`] override if set and non-empty; otherwise
/// the nearest ancestor of the current directory (inclusive) whose
/// `Cargo.toml` declares a `[workspace]` table; otherwise the compile-time
/// parent of `CARGO_MANIFEST_DIR`, announced on stderr because it means the
/// binary is reading `benchmarks/` from the tree it was built in rather
/// than the one it is running in.
pub(crate) fn workspace_root() -> PathBuf {
    let compile_time = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kermit crate must be inside workspace")
        .to_path_buf();
    let cwd = std::env::current_dir().unwrap_or_else(|_| compile_time.clone());
    let env_override = std::env::var(WORKSPACE_ENV).ok();
    let (resolved, source) = resolve_workspace_root(env_override.as_deref(), &cwd, &compile_time);
    if source == RootSource::CompileTime {
        eprintln!(
            "kermit: no workspace Cargo.toml above {}; using compile-time root {}",
            cwd.display(),
            compile_time.display()
        );
    }
    resolved
}

/// Which rule of [`resolve_workspace_root`] produced the root. Reported so
/// [`workspace_root`] can warn only when the compile-time path was used.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RootSource {
    /// [`WORKSPACE_ENV`] was set and non-empty.
    Env,
    /// A `Cargo.toml` with a `[workspace]` table was found above `cwd`.
    WalkUp,
    /// Neither applied; the compile-time parent of `CARGO_MANIFEST_DIR`.
    CompileTime,
}

/// Pure resolution used by [`workspace_root`]: `env_override` is the raw
/// [`WORKSPACE_ENV`] value, `cwd` the directory to walk up from, and
/// `fallback` the compile-time root. Takes its inputs as arguments so it
/// can be tested without touching the process environment or directory.
fn resolve_workspace_root(
    env_override: Option<&str>, cwd: &Path, fallback: &Path,
) -> (PathBuf, RootSource) {
    if let Some(explicit) = env_override.filter(|s| !s.is_empty()) {
        return (PathBuf::from(explicit), RootSource::Env);
    }
    match find_workspace_manifest(cwd) {
        | Some(root) => (root, RootSource::WalkUp),
        | None => (fallback.to_path_buf(), RootSource::CompileTime),
    }
}

/// Walks up from `start` (inclusive) and returns the first directory whose
/// `Cargo.toml` contains a `[workspace]` table header. A substring test is
/// enough here: a member crate's manifest has no such header, and a
/// manifest that only mentions `[workspace]` in a comment is not worth a
/// TOML dependency.
fn find_workspace_manifest(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        let manifest = dir.join("Cargo.toml");
        let text = fs::read_to_string(&manifest).ok()?;
        text.contains("[workspace]").then(|| dir.to_path_buf())
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit materialize::tests::workspace_root`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Confirm the existing CLI tests still reach the workspace through the walk-up**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_run_sweep`
Expected: pass. (The test binary runs with the crate directory as cwd; the walk-up finds the workspace `Cargo.toml` one level up.)

- [ ] **Step 6: Commit**

```bash
git add kermit/src/materialize.rs
git commit -m "feat(kermit): resolve the workspace root at runtime with a KERMIT_WORKSPACE override"
```

---

### Task 5: `ReportSink`

**Files:**
- Modify: `kermit/src/bench_report.rs` — imports, new type after `write_json_report`, tests

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `kermit/src/bench_report.rs`:

```rust
    fn report(kind: BenchKind, tag: &str) -> BenchReport {
        let axes = std::collections::BTreeMap::from([(
            "tag".to_string(),
            serde_json::json!(tag),
        )]);
        BenchReport::new(kind, &[], axes, vec![])
    }

    fn read_tags(path: &std::path::Path) -> Vec<String> {
        let text = std::fs::read_to_string(path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json.as_array()
            .unwrap()
            .iter()
            .map(|r| r["axes"]["tag"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn sink_finish_without_push_writes_empty_array() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/run.json");
        let sink = ReportSink::open(Some(&path), BenchKind::Run).unwrap();
        let written = sink.finish().unwrap();
        assert_eq!(written, path);
        assert_eq!(read_tags(&path), Vec::<String>::new());
    }

    #[test]
    fn sink_push_is_visible_on_disk_before_finish() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        let mut sink = ReportSink::open(Some(&path), BenchKind::Run).unwrap();
        sink.push(vec![report(BenchKind::Run, "cell-1")]).unwrap();
        // The file already holds the first cell and no staging file remains.
        assert_eq!(read_tags(&path), vec!["cell-1"]);
        assert!(!dir.path().join("run.json.part").exists());
        sink.push(vec![
            report(BenchKind::Run, "cell-2a"),
            report(BenchKind::Run, "cell-2b"),
        ])
        .unwrap();
        assert_eq!(read_tags(&path), vec!["cell-1", "cell-2a", "cell-2b"]);
        sink.finish().unwrap();
        assert_eq!(read_tags(&path), vec!["cell-1", "cell-2a", "cell-2b"]);
    }

    #[test]
    fn sink_default_path_uses_kind_and_bench_runs_dir() {
        let path = ReportSink::default_path(BenchKind::Ds);
        assert_eq!(path.parent().unwrap(), std::path::Path::new("bench-runs"));
        let file = path.file_name().unwrap().to_str().unwrap();
        assert!(file.starts_with("ds-"), "{file}");
        assert!(file.ends_with(".json"), "{file}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit bench_report::tests::sink`
Expected: compile error `cannot find struct `ReportSink``.

- [ ] **Step 3: Implement**

Change the `use` block at the top of `kermit/src/bench_report.rs` to:

```rust
use {
    serde::Serialize,
    std::{
        collections::BTreeMap,
        fs,
        io::{self, BufWriter, Write},
        path::{Path, PathBuf},
    },
};
```

Insert after `write_json_report`:

```rust
/// Crash-safe writer for the JSON report of one `bench` invocation.
///
/// `bench run` produces reports one cell at a time, and a sweep can run
/// for hours. The sink rewrites the complete array after every
/// [`push`](Self::push) — to a `.part` sibling, then renamed into place —
/// so at any instant the file on disk is a valid JSON array holding every
/// cell that has finished. An interrupted sweep therefore keeps its
/// completed cells; only the cell in flight is lost.
///
/// `bench join` and `bench ds` produce a single report and use the same
/// sink so all three subcommands share one output path.
pub struct ReportSink {
    path: PathBuf,
    reports: Vec<BenchReport>,
}

impl ReportSink {
    /// Resolves the target path — `override_path` if given, otherwise
    /// [`default_path`](Self::default_path) — and creates its parent
    /// directory. Nothing is written until the first `push` or `finish`.
    pub fn open(override_path: Option<&Path>, kind: BenchKind) -> io::Result<Self> {
        let path = match override_path {
            | Some(p) => p.to_path_buf(),
            | None => Self::default_path(kind),
        };
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        Ok(Self {
            path,
            reports: Vec::new(),
        })
    }

    /// The default report path: `bench-runs/{kind}-{unix-millis}.json`
    /// relative to the current directory (`bench-runs/` is gitignored at
    /// the workspace root).
    pub fn default_path(kind: BenchKind) -> PathBuf {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let kind_str = match kind {
            | BenchKind::Join => "join",
            | BenchKind::Ds => "ds",
            | BenchKind::Run => "run",
        };
        PathBuf::from(format!("bench-runs/{kind_str}-{now_ms}.json"))
    }

    /// Appends `reports` and rewrites the whole file atomically.
    pub fn push(&mut self, reports: Vec<BenchReport>) -> io::Result<()> {
        self.reports.extend(reports);
        self.write_atomically()
    }

    /// Writes the file (even if nothing was pushed, so an empty sweep
    /// still yields `[]`), announces the path on stderr, and returns it.
    pub fn finish(self) -> io::Result<PathBuf> {
        self.write_atomically()?;
        eprintln!("Report written: {}", self.path.display());
        Ok(self.path)
    }

    /// Serialises to `<path>.part` and renames over `<path>`, the same
    /// staging discipline `kermit-bench`'s downloader uses, so a crash
    /// mid-write can never leave a truncated report.
    fn write_atomically(&self) -> io::Result<()> {
        let mut part = self.path.clone().into_os_string();
        part.push(".part");
        let part = PathBuf::from(part);
        {
            let mut writer = BufWriter::new(fs::File::create(&part)?);
            write_json_report(&mut writer, &self.reports)?;
            writer.flush()?;
        }
        fs::rename(&part, &self.path)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit bench_report::tests`
Expected: `test result: ok. 7 passed` (4 existing + 3 new).

- [ ] **Step 5: Commit**

```bash
git add kermit/src/bench_report.rs
git commit -m "feat(kermit): add ReportSink for atomic per-cell report rewrites"
```

---

### Task 6: Route the three handlers through `ReportSink`

**Files:**
- Modify: `kermit/src/main.rs` — imports (~line 35), delete `default_report_path` + `write_bench_report` (~lines 583-612), `run_bench_join` tail (~line 1285), `run_ds_bench_command` (~line 1343), `run_bench_run_command` (~line 1446)

- [ ] **Step 1: Imports**

In the second `use { ... }` block of `kermit/src/main.rs`, change

```rust
    bench_report::{
        write_json_report, write_metadata_block, BenchKind, BenchReport, CriterionGroupRef,
        MetadataLine, ReportMetric,
    },
```

to

```rust
    bench_report::{
        write_metadata_block, BenchKind, BenchReport, CriterionGroupRef, MetadataLine,
        ReportMetric, ReportSink,
    },
```

- [ ] **Step 2: Delete the old helpers**

Delete the whole of `fn default_report_path(kind: BenchKind) -> PathBuf { ... }` and `fn write_bench_report(...) -> anyhow::Result<()> { ... }` from `main.rs`. Their logic now lives in `ReportSink::default_path` / `ReportSink::open` / `ReportSink::finish`.

- [ ] **Step 3: `run_bench_join`**

Replace the tail

```rust
    write_bench_report(
        bench_args.report_json.as_deref(),
        BenchKind::Join,
        std::slice::from_ref(&report),
    )?;
    Ok(())
```

with

```rust
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Join)?;
    sink.push(vec![report])?;
    sink.finish()?;
    Ok(())
```

- [ ] **Step 4: `run_ds_bench_command`**

Replace

```rust
    let mut reports: Vec<BenchReport> = Vec::new();
    for ds in indexstructure.expand() {
        let report = dispatch_ds_bench(
            ds,
            layout.hash_trie_hasher_resolved(),
            layout.hash_trie_pruning_resolved(),
            hash_trie_config,
            &relation,
            &metrics,
            queries_per_build,
            group_name,
            bench_args,
        )?;
        reports.push(report);
    }
    write_bench_report(bench_args.report_json.as_deref(), BenchKind::Ds, &reports)?;
    Ok(())
```

with

```rust
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Ds)?;
    for ds in indexstructure.expand() {
        let report = dispatch_ds_bench(
            ds,
            layout.hash_trie_hasher_resolved(),
            layout.hash_trie_pruning_resolved(),
            hash_trie_config,
            &relation,
            &metrics,
            queries_per_build,
            group_name,
            bench_args,
        )?;
        sink.push(vec![report])?;
    }
    sink.finish()?;
    Ok(())
```

- [ ] **Step 5: `run_bench_run_command`**

Replace

```rust
    let mut reports: Vec<BenchReport> = Vec::new();
    for benchmark in &materialized {
        for &cell in &cells {
            let mut cell_reports = dispatch_run_bench(
                cell,
                benchmark,
                optimiser,
                &metrics,
                queries_per_build,
                query.as_deref(),
                bench_args,
            )?;
            reports.append(&mut cell_reports);
        }
    }
    write_bench_report(bench_args.report_json.as_deref(), BenchKind::Run, &reports)?;
    Ok(())
```

with

```rust
    // Opened before the loop so every finished cell is on disk before the
    // next one starts; a crash mid-sweep keeps the completed cells.
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Run)?;
    for benchmark in &materialized {
        for &cell in &cells {
            let cell_reports = dispatch_run_bench(
                cell,
                benchmark,
                optimiser,
                &metrics,
                queries_per_build,
                query.as_deref(),
                bench_args,
            )?;
            sink.push(cell_reports)?;
        }
    }
    sink.finish()?;
    Ok(())
```

- [ ] **Step 6: Build, fix any now-unused imports, run the CLI suite**

Run: `CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings`
Expected: clean. If `BenchReport` is reported unused in `main.rs`, it is still used by `dispatch_ds_bench` / `run_benchmark` signatures, so it should not be; if `BufWriter` or `fs` become unused, remove them from the first `use` block (`write_tuples` still uses `BufWriter`, `run_ds_bench` still uses `fs::metadata`; verify with the compiler rather than guessing).

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit`
Expected: all pass. Every CLI test passes `--report-json` and parses the file; they exercise `open` → `push` → `finish` for all three subcommands.

- [ ] **Step 7: Commit**

```bash
git add kermit/src/main.rs
git commit -m "feat(kermit): write bench reports incrementally through ReportSink"
```

---

### Task 7: CLI test — a failing second benchmark keeps the first's reports

**Files:**
- Modify: `kermit/tests/cli_bench_run_sweep.rs` (append a test; reuse `kermit_bin`, `fixtures_dir`, `MINI_BENCH`, `make_fake_cache`, `skip_unsupported`)

Mechanism: `KERMIT_WORKSPACE` (Task 4) points at an empty temp dir so no workspace YAML is discovered; `XDG_CACHE_HOME` points at a fake cache holding two entries. Cache subdirs are visited in filename order (`load_all_benchmarks_with_cache` sorts them), so `a-ok` runs first and `z-bad` fails inside `run_benchmark` at `ensure_cached`, because its relation file is absent and its `file://` URL cannot be downloaded.

- [ ] **Step 1: Write the failing test**

Append to `kermit/tests/cli_bench_run_sweep.rs`:

```rust
/// Builds a fake cache with two static benchmarks: `a-ok` (the mini
/// fixture, relations present) and `z-bad` (one relation whose parquet is
/// missing and whose URL cannot be fetched). Filename order guarantees
/// `a-ok` runs first.
fn make_two_benchmark_cache() -> TempDir {
    let tmp = tempfile::tempdir().expect("failed to create temp cache dir");
    let root = tmp.path().join("kermit/benchmarks");
    let artifacts = fixtures_dir().join("watdiv-mini/artifacts");

    let ok_dir = root.join("a-ok");
    fs::create_dir_all(&ok_dir).unwrap();
    let yaml = fs::read_to_string(artifacts.join(format!("{MINI_BENCH}.yml"))).unwrap();
    fs::write(
        ok_dir.join("benchmark.yml"),
        yaml.replacen(&format!("name: {MINI_BENCH}"), "name: a-ok", 1),
    )
    .unwrap();
    fs::write(ok_dir.join("meta.json"), "{}").unwrap();
    for rel in ["eligibleregion", "includes", "parentcountry"] {
        fs::copy(
            artifacts.join(format!("{rel}.parquet")),
            ok_dir.join(format!("{rel}.parquet")),
        )
        .unwrap();
    }

    let bad_dir = root.join("z-bad");
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(
        bad_dir.join("benchmark.yml"),
        "name: z-bad\n\
         description: relation cannot be fetched\n\
         relations:\n\
         - name: missing\n  \
           url: file:///nonexistent/kermit-test/missing.parquet\n\
         queries:\n\
         - name: q0002\n  \
           description: named q0002 so the -q filter below passes and the failure is the fetch\n  \
           query: 'Q(X, Y) :- missing(X, Y).'\n",
    )
    .unwrap();
    fs::write(bad_dir.join("meta.json"), "{}").unwrap();
    tmp
}

#[test]
fn partial_sweep_keeps_reports_of_finished_benchmarks() {
    if skip_unsupported() {
        return;
    }
    let cache = make_two_benchmark_cache();
    let empty_workspace = tempfile::tempdir().expect("failed to create temp workspace");
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let output = Command::new(kermit_bin())
        .env("XDG_CACHE_HOME", cache.path())
        .env("KERMIT_WORKSPACE", empty_workspace.path())
        .args([
            "bench",
            "--sample-size",
            "10",
            "--measurement-time",
            "1",
            "--warm-up-time",
            "1",
            "--report-json",
        ])
        .arg(report.path())
        .args([
            "run",
            "--all",
            "-q",
            "q0002",
            "--indexstructure",
            "tree-trie",
            "--algorithm",
            "leapfrog-triejoin",
            "--metrics",
            "iteration",
        ])
        .output()
        .expect("failed to execute kermit binary");

    assert!(
        !output.status.success(),
        "z-bad must fail the run; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(report.path()).expect("report file should exist");
    let reports: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("report must be a valid JSON array after a failure");
    assert_eq!(reports.len(), 1, "exactly a-ok's single-query report: {text}");
    assert_eq!(reports[0]["axes"]["benchmark"], "a-ok");
    assert_eq!(reports[0]["axes"]["query"], "q0002");
    let staging = PathBuf::from(format!("{}.part", report.path().display()));
    assert!(!staging.exists(), "no staging file may remain: {}", staging.display());
}
```

Why `z-bad`'s query is named `q0002`: `run_benchmark` applies the `-q` filter *before* `ensure_cached`. With a matching query name the failure is the fetch of the missing relation, which is the path this test is about. (A non-matching name would also fail after `a-ok`'s push, but on the filter instead.)

- [ ] **Step 2: Mutation check — prove the test can fail**

Temporarily reintroduce the old behaviour in `run_bench_run_command`: inside the loop replace `sink.push(cell_reports)?;` with `pending.extend(cell_reports);` (declare `let mut pending: Vec<BenchReport> = Vec::new();` before the loop) and after the loop insert `sink.push(pending)?;` before `sink.finish()?;`. Confirm with `git diff kermit/src/main.rs` that the mutation applied, then run:

`CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_run_sweep partial_sweep`

Expected: FAIL at `report must be a valid JSON array after a failure` (the file is empty because nothing was written before the error). Then revert with `git checkout kermit/src/main.rs` and confirm `git diff --stat` is empty.

- [ ] **Step 3: Run the test against the real implementation**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_run_sweep partial_sweep`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add kermit/tests/cli_bench_run_sweep.rs
git commit -m "test(kermit): partial bench run sweep keeps finished benchmarks' reports"
```

---

### Task 8: Full gate and manual interrupt check

- [ ] **Step 1: Format inside the flake**

Run: `nix develop --command cargo fmt --all`
Then: `git diff --stat` — if fmt changed files, commit them with `style: rustfmt`.

- [ ] **Step 2: The CI gate**

Run, each in the foreground:

```bash
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets --verbose
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
CARGO_BUILD_JOBS=2 cargo test --verbose
nix develop --command cargo fmt --all --check
```

Expected: all clean. Miri is unaffected (no unsafe, no new crates) but if time allows run `nix develop --command bash -c 'MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test -p kermit-ds -p kermit-algos'` since those two crates gained code.

- [ ] **Step 3: Manual interrupt check (spec § Verification)**

```bash
CARGO_BUILD_JOBS=2 cargo run -- bench --measurement-time 3 --warm-up-time 1 --report-json /tmp/claude-1000/sweep.json run triangle -i all -a all -m iteration
```

Watch stderr; when the second cell's metadata block appears, press Ctrl-C. Then:

```bash
python3 -c "import json; r=json.load(open('/tmp/claude-1000/sweep.json')); print(len(r), [x['axes']['data_structure'] for x in r])"
```

Expected: `1 ['ColumnTrie']` (or whichever cell ran first) and no `/tmp/claude-1000/sweep.json.part`.

- [ ] **Step 4: Update the spec status and commit**

Change `**Status:** Approved design, not yet implemented` in `docs/specs/2026-09-09-sweep-hardening-design.md` to `**Status:** Implemented 2026-09-09`, and add one line under section 2's Tests noting that resolution was made a pure function (`resolve_workspace_root`) instead of using a mutex.

```bash
git add docs/specs/2026-09-09-sweep-hardening-design.md
git commit -m "docs(specs): mark sweep-hardening as implemented"
```
