# Sweep Hardening

**Date:** 2026-09-09
**Status:** Approved design, not yet implemented
**Scope:** Sub-project A of the benchmarking-flow improvement series
(A: sweep hardening, B: runner consolidation, C: answer verification,
D: Rust–Python contract, E: download integrity).

## Motivation

Three findings from the 2026-09-09 architectural analysis of the
benchmarking flow, all in the `bench run` handler area, all small, and all
protecting measurements that have already been paid for:

1. `run_bench_run_command` (`kermit/src/main.rs`) writes the JSON report
   once, after every cell of the sweep has finished. A crash or interrupt in
   cell 3 of an hours-long `-i all -a all` sweep loses the reports for cells
   1 and 2, even though their `target/criterion/` directories exist.
2. `materialize::workspace_root` is baked in at compile time via
   `env!("CARGO_MANIFEST_DIR")`, while `bench-runs/` and `target/criterion/`
   are relative to the current directory. A binary built in one worktree and
   run from another reads that worktree's `benchmarks/` but writes results
   locally.
3. The `data_structure` / `algorithm` strings that name Criterion
   directories and report axes are `format!("{:?}", ...)` on
   `IndexStructure` / `JoinAlgorithm`, computed independently at three sites
   in `main.rs`. Comments call this a stable external contract, but nothing
   enforces it: a derive change silently repartitions history.

None of these change the contents of a report or the name of a Criterion
group for a run that completes normally.

## 1. Incremental report writing

### Design

A `ReportSink` in `kermit/src/bench_report.rs`:

```rust
pub struct ReportSink {
    path: PathBuf,
    reports: Vec<BenchReport>,
}

impl ReportSink {
    /// Resolves the target path (override or `default_report_path(kind)`)
    /// and creates its parent directory. Writes nothing yet.
    pub fn open(override_path: Option<&Path>, kind: BenchKind) -> io::Result<Self>;

    /// Appends `reports` and rewrites the whole file atomically.
    pub fn push(&mut self, reports: Vec<BenchReport>) -> io::Result<()>;

    /// Writes the file even if `push` was never called (an empty sweep
    /// still produces `[]`, as today), prints `Report written: <path>`,
    /// and returns the path.
    pub fn finish(self) -> io::Result<PathBuf>;
}
```

The atomic rewrite serialises the full array to `<path>.part` and renames it
over `<path>`, the same staging discipline `kermit-bench`'s `download_file`
uses. Between any two cells the file on disk is therefore a complete, valid
JSON array holding every finished cell.

`run_bench_run_command` opens the sink before the loop, calls `push` after
each `dispatch_run_bench`, and calls `finish` after the loop. `run_bench_join`
and `run_ds_bench_command` produce exactly one report and switch to
`open` + `push` + `finish` for uniformity; `write_bench_report` and
`default_report_path` move into `bench_report.rs` as private helpers of the
sink.

No signal handling is added. Ctrl-C between cells leaves a valid file; Ctrl-C
during a rename is handled by the rename's atomicity; Ctrl-C during a cell
loses only that cell, which is the best achievable without checkpointing
inside Criterion.

### Unchanged

Output path, default name, JSON shape, `--report-json` semantics,
`schema_version`. `kermit-lab` needs no change.

### Tests

- Unit (`bench_report.rs`): `push` twice then `finish` yields one array with
  both batches; `finish` with no `push` yields `[]`; after `push` the file on
  disk already parses to the pushed reports and no `.part` file remains.
- CLI (`kermit/tests/cli_bench_run_sweep.rs`): point the binary at a temp
  workspace via `KERMIT_WORKSPACE` (section 2) containing two static
  benchmarks: `a-ok.yml`, whose relation is a local `path:` to a copied
  fixture, and `z-bad.yml`, whose relation `url:` is a `file://` path that
  does not exist. Discovery sorts by filename, so `bench run --all
  -i tree-trie -a leapfrog-triejoin` runs `a-ok` to completion and then
  fails inside `run_benchmark` for `z-bad` at `ensure_cached`. Assert the
  process exits non-zero, the report file parses, and it holds exactly
  `a-ok`'s reports. This test depends on section 2 landing first.

## 2. Runtime workspace-root resolution

### Design

`materialize::workspace_root()` resolves in this order:

1. `KERMIT_WORKSPACE` environment variable, if set and non-empty, taken
   verbatim.
2. Walk up from `std::env::current_dir()` looking for a `Cargo.toml` whose
   text contains a `[workspace]` table header; the first hit's directory.
3. The compile-time parent of `CARGO_MANIFEST_DIR`, with a one-line stderr
   note that the compile-time path was used.

The return type stays `PathBuf`; the function does not become fallible. Step 3
guarantees a result, so no caller changes. `vendored_watdiv_root` and
`vendored_lubm_jar` derive from it unchanged.

The `[workspace]` check is a substring test on the file text, not a TOML
parse: it needs no new dependency and a false positive requires a
`Cargo.toml` that mentions `[workspace]` in a comment, which is not worth
guarding against.

### Tests

- Unit: a temp tree `root/Cargo.toml` (with `[workspace]`) and
  `root/a/b/`; with the current directory set to `root/a/b` the walk-up
  returns `root`. A second test sets `KERMIT_WORKSPACE` to a temp path and
  asserts it wins over the walk-up. Both tests must serialise on the process
  environment and current directory (a `static Mutex` in the test module),
  since Rust tests run in parallel.
- Existing CLI tests run from the crate directory and reach the workspace
  through step 2; no change.

## 3. Stable identity labels

### Design

`IndexStructure` (`kermit-ds/src/ds/mod.rs`) and `JoinAlgorithm`
(`kermit-algos/src/lib.rs`) gain:

```rust
pub fn axis_value(self) -> &'static str
```

returning `"TreeTrie"`, `"ColumnTrie"`, `"HashTrie"` and `"LeapfrogTriejoin"`,
`"HashTriejoin"` respectively — the exact strings the `Debug` derive produces
today, so no existing Criterion directory or report changes. This mirrors
`Optimiser::axis_value` and its `axis_values_match_clap_value_names` guard.

The three `format!("{:?}", ...)` sites in `main.rs` (`run_ds_bench`,
`run_benchmark`, `run_bench_join`) switch to `axis_value()`. The comments
that currently explain the `Debug` contract are replaced by a pointer to
`axis_value`.

`docs/specs/bench-report-schema.md` changes the `data_structure` and
`algorithm` rows from "matches the `Debug` repr" to "the `axis_value`
string".

### Tests

- One test per enum pinning every variant's `axis_value` against a string
  literal, so a rename fails loudly.
- One test per enum asserting `axis_value() == format!("{:?}", v)` for every
  variant, with a message explaining that diverging the two is allowed only
  with a deliberate decision about historical measurements.

## Out of scope

- Any change to report contents, axis keys, or Criterion group/function
  naming (including the `bench ds` vs `bench run` function-name asymmetry).
- Moving the runners out of `main.rs` (sub-project B).
- Checkpointing inside a cell.

## Verification

`cargo test`, `cargo clippy --all-targets` with `-Dwarnings`,
`cargo fmt --all --check` inside `nix develop`, `cargo doc`. For section 1,
additionally run a real two-cell `bench run triangle -i all -a all` with a
short `--measurement-time`, interrupt it by hand during the second cell, and
confirm the report file parses and holds the first cell.
