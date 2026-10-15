# Shared Generator Layer (#95, push 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the non-RDF generator layer that #95 (H2O), #96 (SNAP) and #97 (JOB) all build on, as its own push to master before any generator: a streaming, batch-independent Parquet writer in kermit-ds, and a new workspace crate `kermit-generate` holding the `Generator` trait, the `RelationSink` that writes and hashes relations, and `run`, which owns the generated-cache contract.

**Architecture:**
- **Story 1, `ParquetWriter` (kermit-ds).** `kermit_ds::{ParquetWriter, write_parquet}` in a new `parquet_writer.rs`. It is the exact inverse of `read_parquet` and streams (`create` / `write_batch(&Tuples)` / `finish`). Rows are re-chunked into fixed 2¹⁶-row column buffers, so a file's bytes don't depend on the caller's batching. It writes `<name>.parquet.part` and renames it on `finish`; a dropped writer leaves nothing behind.
- **Story 2, the crate.** Workspace member `kermit-generate` (manifest, crate docs, `#![deny(missing_docs)]`) and `output_dir`, the pure resolver for `bench gen <kind>`'s `--output-dir` (D17). kermit-rdf's `utc_iso8601_now` becomes public, and the binary's manifest gains the dependency, unused until push 2.
- **Story 3, the layer.** `RelationSink` / `RelationWriter` / `RelationRecord`, then `GeneratedMeta<E>` (the common `meta.json`, with the extras flattened), then `Generator`, `Generated` and `run`. `run` deletes the old `meta.json`, generates, checks the extras and queries, writes `benchmark.yml`, and writes `meta.json` last. Integration tests drive a configurable toy generator through the public API.
- **Story 4, docs.** CLAUDE.md and ARCHITECTURE.md list the ninth crate, its edges and its contract.
- **Untouched:** every data structure and algorithm, the binary's source (`kermit/src`), the bench report schema, and kermit-rdf beyond one visibility change. No `bench gen` kind and no `dispatch` arm land here; those are push 2's (#95) or #96's.

**Tech Stack:** Rust nightly (workspace `rust-toolchain.toml`) in the nix dev shell; `parquet` / `arrow` 59.3.0 (`ArrowWriter`, `WriterProperties`); `serde` / `serde_json` / `serde_yaml`; `anyhow`; `tempfile` for tests.

**Spec:** `docs/specs/2026-10-15-h2o-benchmark-design.md`

**Plan review:** code-quality-reviewer, 2026-10-15, read-only against spec 41537a4 and contract f7e21c1 (no cargo; the host was timing). No critical findings; 2 important and 9 minor findings were fixed before commit (Merge notes, "Plan review"), and 1 decision was settled: an empty batch is a no-op at any arity.

The spec is at 41537a4; Part 1 and decisions D13–D17 apply. **Names contract:** `docs/superpowers/plans/2026-10-15-generate-layer-contract.md` (f7e21c1). #96 and #97 take their `kermit-generate` names from that contract, so this plan must not rename anything it fixes. A needed deviation goes back to the design session; it is never improvised.

---

## Ground rules for the executor

- **Paths.** `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/h2o_18dea44a38dbca15`. Every command runs from `$WT`. `BASE` is the commit that adds this plan; the scope checks diff against it.
- **Branch:** the loom branch `aidanb/h2o`. Never switch branches, never amend, never push. Pushing is "After this plan", and it needs the user's go-ahead.
- **Before Story 1:** if this plan is not committed yet, commit it by name: `git add docs/superpowers/plans/2026-10-15-generate-layer.md`, message `docs: plan #95 push 1, the kermit-generate layer`, with the trailer lines below.
- **Host: no cargo until the #101 replicate chain has finished.** The benchmark co-ordinator session (`aidanb/benchmark-co-ordinator`) announces `FULL_EXIT` (in `kermit-bench-runs/hash-trie-chain-links-2026-10-14/logs/full.log`) and allots this plan's gate slot. Before any compile or test, both of these must print nothing:
  ```bash
  pgrep -af "[k]ermit-bench-runs/.*\.sh"
  pgrep -af "[k]ermit-bench-runs/.*/bin/kermit"
  ```
  The first matters most: a chain's driver scripts live across the ~10 s gaps between its benchmark binaries. If either prints anything, stop and report; a compile under a timing run contaminates it. If the chain is still live, the tasks may be applied as edits and verified later, in task order: save each task's diff as a patch, then replay, build, test and commit task by task (the "edit now, verify later" recipe in the host-sharing memory). Nothing unverified is committed.
- **Builds:** every cargo command runs inside the dev shell with two jobs, in the foreground: `nix develop --command env CARGO_BUILD_JOBS=2 cargo …`. Set `RUSTFLAGS` / `RUSTDOCFLAGS` inside the `env`, never outside `nix develop`:
  - clippy: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets`;
  - doc: `nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`.

  The full workspace test (final gate) runs through `setsid nohup … & disown`, with a `kill -0` watcher.
- **Test runner:** `cargo test`. The dev shell has no `cargo-nextest`.
- **Formatting:** `nix develop --command cargo fmt --all` only. Stable rustfmt rewrites ~30 files.
- **Doc comments:** backtick every code identifier (clippy `doc_markdown` runs under `-Dwarnings`). `kermit-ds` and `kermit-generate` have `#![deny(missing_docs)]`. `wrap_comments` wraps at 80 columns, so accept `cargo fmt`'s re-wrapping.
- **No `unsafe`.**
- **Staging and committing by path only.** Never `git add -A` / `-u` / `.` / a directory, never `git commit -a` / `--amend`, and commit with a pathspec: `git commit -m "…" -- <files>`. The developer agents' guard hook enforces this and blocks `git reset`, `stash`, `checkout`, `restore`, `clean`, `rebase`, `switch` and `push`. The hook runs `scripts/hooks/developer_git_guard.py`, an **untracked copy** of the roster's guard placed in this worktree for this plan and listed in `.git/info/exclude` (with `.superpowers/`). Never commit, edit or delete it.
- **Commit trailers**, on every commit:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD
  ```

## File map

Derived from the contract the stories were drafted against. Each task's own **Files** list is authoritative.

| File | Story | Change |
|---|---|---|
| `kermit-ds/src/parquet_writer.rs` | 1 | new: `ParquetWriter`, `write_parquet`, `WRITE_CHUNK_ROWS`, `ROW_GROUP_ROWS`, tests |
| `kermit-ds/src/lib.rs` | 1 | `mod parquet_writer;`, re-exports |
| `kermit-ds/src/relation.rs` | 1 | `file_stem` → `pub(crate)`; test helper `write_parquet` → `write_i64_parquet` |
| `kermit-rdf/src/lib.rs`, `kermit-rdf/src/timestamp.rs` | 2 | `pub mod timestamp`, `pub fn utc_iso8601_now` |
| `Cargo.toml` | 2 | workspace member and `[patch.crates-io]` entry |
| `kermit/Cargo.toml` | 2 | `kermit-generate` dependency |
| `kermit-generate/Cargo.toml`, `src/lib.rs`, `src/output_dir.rs` | 2 | new crate, crate docs, `output_dir` / `OutputDir` |
| `kermit-generate/src/sink.rs` | 3 | `RelationSink`, `RelationWriter`, `RelationRecord` |
| `kermit-generate/src/meta.rs` | 3 | `GeneratedMeta<E>`, `COMMON_FIELDS` |
| `kermit-generate/src/generator.rs`, `src/run.rs` | 3 | `Generator`, `Generated`, `run` |
| `kermit-generate/tests/run.rs` | 3 | integration tests with the toy generator |
| `CLAUDE.md`, `ARCHITECTURE.md` | 4 | the ninth crate |

## Execution order

Stories run in order: 1 → 2 → 3 → 4, then the final gate. Story 3 uses Story 1's `ParquetWriter` and Story 2's crate. Every code story builds with cargo, so none of them run concurrently. Story 4 runs no cargo and may run alongside any of them.

## Story 1: `ParquetWriter` and `write_parquet` in kermit-ds

- **Developer:** intermediate-dev
- **Files:** `kermit-ds/src/parquet_writer.rs`, `kermit-ds/src/lib.rs`, `kermit-ds/src/relation.rs`
- **Interfaces:** exposes `kermit_ds::ParquetWriter` (`create`, `write_batch`, `finish`) and `kermit_ds::write_parquet`, re-exported at the crate root beside `read_parquet`; uses `RelationHeader`, `RelationError`, `read_parquet` and `Tuples` from the tree as it stands
- **Verify:** `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds parquet`

`kermit-ds` can read a relation from Parquet but cannot write one, and the generators of #95/#96/#97 must. This story adds `ParquetWriter`, a streaming writer, and `write_parquet`, its one-shot form, in a new module `kermit-ds/src/parquet_writer.rs` (as the spec says since 41537a4: a new module keeps the 1047-line `relation.rs` from growing). The writer is the exact inverse of `read_parquet`: one non-nullable `Int64` column per attribute, and a file stem that must equal the header's name. It re-chunks caller rows into fixed `WRITE_CHUNK_ROWS` column buffers so the bytes do not depend on how the caller batched, writes to `<path>.part` and renames on `finish` so no partial file ever carries a relation's name, and refuses further batches after any error. Task 1.1 first renames the raw-`i64` test helper in `relation.rs` that would otherwise clash with the new public name. No `unsafe`; no new dependency (`arrow`, `parquet` and `tempfile` are already in `kermit-ds/Cargo.toml`). Nothing in this story touches a join path or an index structure, so no `effort: xhigh` dispatch is needed; the shapes a naive suite would miss (page cuts that depend on batching, a `Drop` that must remove `.part`, the row-group boundary) each get a mutation check in Task 1.3.

### Task 1.1: Rename relation.rs's raw-`i64` test helper

`relation.rs`'s `#[cfg(test)] mod tests` has a private `fn write_parquet(path, attrs, rows: &[Vec<i64>])` that writes any `i64`, including negative values `read_parquet` rejects. The public `kermit_ds::write_parquet` of Task 1.2 takes `usize` tuples and could never write those, so the helper stays, under the name `write_i64_parquet`. (`kermit-ds/tests/common/macros.rs` has its own, unrelated `write_parquet` inside a macro in the integration-test crate; it is left alone.)

**Files:**
- Modify: `kermit-ds/src/relation.rs:747-774`
- Modify: `kermit-ds/src/relation.rs:801`
- Modify: `kermit-ds/src/relation.rs:813`
- Modify: `kermit-ds/src/relation.rs:849`
- Modify: `kermit-ds/src/relation.rs:871`
- Modify: `kermit-ds/src/relation.rs:887`
- Test: `kermit-ds/src/relation.rs` (the existing `mod tests`)

- [ ] **Step 1: Write the failing check** (a rename has no new test; the check is that the new name is defined)

```bash
grep -c 'fn write_i64_parquet' kermit-ds/src/relation.rs
```

- [ ] **Step 2: Run it to see it fail**

Run the command above. It prints `0`.

- [ ] **Step 3: Implement**

In `kermit-ds/src/relation.rs`, make these six edits and no other.

Current (lines 747-748):

```rust
    /// Writes `rows` as an `Int64` Parquet file with one column per name.
    fn write_parquet(path: &Path, attrs: &[&str], rows: &[Vec<i64>]) {
```

Replace with:

```rust
    /// Writes `rows` as an `Int64` Parquet file with one column per name.
    /// Unlike the public `write_parquet`, it writes any `i64`, including the
    /// negative values `read_parquet` rejects.
    fn write_i64_parquet(path: &Path, attrs: &[&str], rows: &[Vec<i64>]) {
```

Current (line 801):

```rust
        write_parquet(&path, &["a", "b", "c"], &[vec![1, 2, 3], vec![4, 5, 6]]);
```

Replace with:

```rust
        write_i64_parquet(&path, &["a", "b", "c"], &[vec![1, 2, 3], vec![4, 5, 6]]);
```

Current (line 813):

```rust
        write_parquet(&path, &["s", "o"], &[]);
```

Replace with:

```rust
        write_i64_parquet(&path, &["s", "o"], &[]);
```

Current (line 849):

```rust
        write_parquet(&parquet, &["a", "b"], &rows);
```

Replace with:

```rust
        write_i64_parquet(&parquet, &["a", "b"], &rows);
```

Current (line 871):

```rust
        write_parquet(&path, &["s", "p", "o"], &rows);
```

Replace with:

```rust
        write_i64_parquet(&path, &["s", "p", "o"], &rows);
```

Current (line 887):

```rust
        write_parquet(&path, &["a", "b"], &[vec![1, 2], vec![3, -4]]);
```

Replace with:

```rust
        write_i64_parquet(&path, &["a", "b"], &[vec![1, 2], vec![3, -4]]);
```

- [ ] **Step 4: Run it to see it pass**

```bash
grep -c 'fn write_i64_parquet' kermit-ds/src/relation.rs
grep -n '[^_]write_parquet(' kermit-ds/src/relation.rs
nix develop --command cargo fmt --all
nix develop --command cargo fmt --all --check
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds relation::tests
nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-ds --all-targets -- -D warnings
```

The first prints `1`. The second prints nothing (no call of the old name is left; `grep` exits 1). `cargo fmt --all` may re-wrap the longer lines; accept what it writes. The test run ends with every `relation::tests` test passing, e.g. `test result: ok. 29 passed; 0 failed` (the count is whatever the module holds), and clippy is clean.

- [ ] **Step 5: Commit**

```bash
git commit -m "refactor(kermit-ds): name relation.rs's raw-i64 Parquet test helper write_i64_parquet

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-ds/src/relation.rs
```

### Task 1.2: `ParquetWriter`, `write_parquet`, round trips and rejections

The writer itself: `create`, `write_batch`, `finish`, `Drop`, and `write_parquet`. Its tests here are the round trips (arities 1, 2, 3 and zero rows) and the four rejections. Task 1.3 adds the tests whose subject is batching, row groups, dropping and failure.

Facts of the parquet 59.3.0 source this code relies on (read, not run):
- `WriterPropertiesBuilder::set_max_row_group_size` is `#[deprecated(since = "58.0.0")]` (`parquet-59.3.0/src/file/properties.rs:741`); its replacement is `set_max_row_group_row_count(Option<usize>)` (`properties.rs:755`), so the code uses that one (clippy runs under `-D warnings`). The default is already `Some(1024 * 1024)` (`properties.rs:618`); the code pins it anyway so a change of default cannot move row-group boundaries.
- `ArrowWriter::try_new(writer, SchemaRef, Option<WriterProperties>)` (`arrow/arrow_writer/mod.rs:227`); `write(&RecordBatch)` returns early on a zero-row batch and splits a batch that would pass `max_row_group_row_count` (`:358-380`); `close(self)` finishes the file (`:505`). Neither `ArrowWriter` nor `SerializedFileWriter` implements `Drop`, so dropping an unclosed writer writes nothing more.
- Data pages are cut by `data_page_row_count_limit` (20 000 rows) checked per `write_batch_size` (1 024) mini-batch counted from the start of each write call (`column/writer/mod.rs:603-605`, `:1093`). That is why the writer re-chunks: its input to `ArrowWriter` must depend only on the row sequence.
- `RecordBatch::try_new(SchemaRef, Vec<ArrayRef>)` (`arrow-array-59.3.0/src/record_batch.rs:263`) rejects a null-free column against a nullable field never, and a mismatch of lengths or types always; `Int64Array: From<Vec<i64>>`.
- Footer row-group count: `ParquetRecordBatchReaderBuilder::metadata()` gives `&Arc<ParquetMetaData>`, which has `num_row_groups()` (`file/metadata/mod.rs:238`) and `row_group(i)`, whose `RowGroupMetaData::num_rows()` is an `i64` (`:669`).

**Files:**
- Create: `kermit-ds/src/parquet_writer.rs`
- Modify: `kermit-ds/src/lib.rs:28-29`
- Modify: `kermit-ds/src/lib.rs:51-52`
- Modify: `kermit-ds/src/relation.rs:428`
- Test: `kermit-ds/src/parquet_writer.rs`

- [ ] **Step 1: Write the failing test**

Create `kermit-ds/src/parquet_writer.rs` holding only its test module (Step 3 puts the implementation above it):

```rust
#[cfg(test)]
mod tests {
    use {super::*, crate::read_parquet};

    /// A header named `name` with attributes `c0`, `c1`, ...
    fn header_of(name: &str, arity: usize) -> RelationHeader {
        RelationHeader::new(name, (0..arity).map(|c| format!("c{c}")).collect())
    }

    /// `rows` deterministic rows of `arity` values, varied enough that
    /// neighbouring rows and columns differ.
    fn rows_of(arity: usize, rows: usize) -> Tuples {
        let mut tuples = Tuples::with_capacity(arity, rows);
        for r in 0..rows {
            let row: Vec<usize> = (0..arity)
                .map(|c| (r * 2_654_435_761 + c * 40_503) % 1_000_003)
                .collect();
            tuples.push(&row);
        }
        tuples
    }

    /// The message of an `InvalidData` error; any other outcome panics.
    /// (`ParquetWriter` is not `Debug`, so `unwrap_err` is unavailable.)
    fn invalid_message<T>(result: Result<T, RelationError>) -> String {
        match result.err() {
            | Some(RelationError::InvalidData(message)) => message,
            | other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn round_trips_arities_1_2_3() {
        let dir = tempfile::tempdir().unwrap();
        for arity in 1..=3 {
            let name = format!("rel{arity}");
            let header = header_of(&name, arity);
            let tuples = rows_of(arity, 2_500);
            let path = dir.path().join(format!("{name}.parquet"));
            write_parquet(&path, &header, &tuples).unwrap();
            assert!(
                !dir.path().join(format!("{name}.parquet.part")).exists(),
                "arity {arity}: finish leaves no .part file"
            );
            assert_eq!(read_parquet(&path).unwrap(), (header, tuples), "arity {arity}");
        }
    }

    #[test]
    fn round_trips_zero_rows() {
        let dir = tempfile::tempdir().unwrap();
        let header = header_of("empty", 2);
        let tuples = Tuples::new(2);
        let path = dir.path().join("empty.parquet");
        write_parquet(&path, &header, &tuples).unwrap();
        let (read_header, read_tuples) = read_parquet(&path).unwrap();
        assert_eq!(read_header, header);
        assert_eq!(read_tuples.arity(), 2);
        assert!(read_tuples.is_empty());
    }

    #[test]
    fn rejects_positional_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.parquet");
        let message =
            invalid_message(ParquetWriter::create(&path, &RelationHeader::new_positional("p", 2)));
        assert!(message.contains("named"), "{message}");
        let message =
            invalid_message(ParquetWriter::create(&path, &RelationHeader::new_positional("p", 0)));
        assert!(message.contains("named"), "{message}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn rejects_stem_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.parquet");
        let message = invalid_message(ParquetWriter::create(&path, &header_of("edge", 2)));
        assert!(message.contains("other"), "{message}");
        assert!(message.contains("edge"), "{message}");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "a rejected create makes no file"
        );
    }

    #[test]
    fn rejects_batch_arity_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
        let message = invalid_message(writer.write_batch(&rows_of(3, 4)));
        assert!(message.contains('3'), "{message}");
        assert!(message.contains('2'), "{message}");
    }

    #[test]
    fn empty_batch_of_any_arity_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        let header = header_of("edge", 2);
        let mut writer = ParquetWriter::create(&path, &header).unwrap();
        writer.write_batch(&Tuples::new(3)).unwrap();
        writer.write_batch(&rows_of(2, 4)).unwrap();
        writer.write_batch(&Tuples::new(0)).unwrap();
        assert_eq!(writer.finish().unwrap(), 4);
        assert_eq!(read_parquet(&path).unwrap(), (header, rows_of(2, 4)));
    }

    #[test]
    fn rejects_value_above_i64_max() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
        writer.write_batch(&rows_of(2, 2)).unwrap();
        // Row 2 overall (row 0 of this batch), column 1.
        let message = invalid_message(writer.write_batch(&Tuples::from(vec![vec![1, usize::MAX]])));
        assert!(message.starts_with("row 2, column 1:"), "{message}");
        assert!(message.contains("exceeds i64::MAX"), "{message}");
    }
}
```

In `kermit-ds/src/lib.rs`, declare the module.

Current (lines 28-29):

```rust
mod morsel;
mod relation;
```

Replace with:

```rust
mod morsel;
mod parquet_writer;
mod relation;
```

- [ ] **Step 2: Run it to see it fail**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds parquet_writer
```

The test build fails: `error[E0425]: cannot find function \`write_parquet\` in this scope`, `error[E0433]: failed to resolve: use of undeclared type \`ParquetWriter\``, and the same for `RelationHeader`, `Tuples` and `RelationError`. No test runs.

- [ ] **Step 3: Implement**

At the top of `kermit-ds/src/parquet_writer.rs`, above the `#[cfg(test)]` line, write the implementation:

```rust
//! A streaming Parquet writer for relations: [`ParquetWriter`], and
//! [`write_parquet`] for a relation held whole.
//!
//! The counterpart of [`read_parquet`](crate::read_parquet), which it
//! inverts exactly.
use {
    crate::relation::{file_stem, RelationError, RelationHeader},
    arrow::{
        array::{ArrayRef, Int64Array, RecordBatch},
        datatypes::{DataType, Field, Schema, SchemaRef},
    },
    kermit_iters::{RowId, Tuples},
    parquet::{arrow::ArrowWriter, file::properties::WriterProperties},
    std::{
        fs::File,
        path::{Path, PathBuf},
        sync::Arc,
    },
};

/// Rows per chunk handed to `ArrowWriter`: the writer's own buffering
/// (`ArrowWriter` also holds the encoded row group in progress).
pub(crate) const WRITE_CHUNK_ROWS: usize = 1 << 16;
/// Rows per row group; a multiple of `WRITE_CHUNK_ROWS`.
pub(crate) const ROW_GROUP_ROWS: usize = 1 << 20;

/// Writes one relation to a Parquet file, batch by batch.
///
/// Four guarantees, the first three each pinned by a test:
///
/// - **The exact inverse of `read_parquet`.** The file has one non-nullable
///   `Int64` column per attribute, named as in the header, and `create`
///   requires the file stem to equal the header's name, so every successful
///   write satisfies `read_parquet(path) == (header, tuples)`.
/// - **Bytes independent of batching.** `ArrowWriter` fixes row-group
///   boundaries but cuts data pages at checkpoints counted from the start of
///   each input batch, so different batchings can give different bytes. The
///   writer therefore transposes caller rows into its own column buffers of
///   `WRITE_CHUNK_ROWS` rows and hands `ArrowWriter` only full chunks, plus
///   the remainder at `finish`. Rows per row group is pinned to
///   `ROW_GROUP_ROWS`, a multiple of the chunk size, so a chunk never
///   straddles a row group. `ArrowWriter`'s input then depends only on the
///   row sequence, and the writer's extra memory is one chunk whatever the
///   caller's batch size.
/// - **No partial files.** Rows go to `<path>.part`, renamed to `path` by
///   `finish`. A writer dropped before `finish`, or one whose `finish`
///   fails, removes `<path>.part` and never writes `path`.
/// - **Stable bytes for a fixed build only.** The footer records
///   `created_by`, which names the parquet crate version, so files differ
///   across versions of it.
///
/// After any error every later call returns an error too.
pub struct ParquetWriter {
    /// `None` once finished or dropped.
    writer: Option<ArrowWriter<File>>,
    schema: SchemaRef,
    /// One chunk buffer per column, each with capacity `WRITE_CHUNK_ROWS`.
    columns: Vec<Vec<i64>>,
    /// Rows currently in `columns`.
    buffered: usize,
    /// Rows accepted so far, across all batches.
    rows: u64,
    /// The final path.
    path: PathBuf,
    /// `<path>.part`: `path` with `.part` appended to its file name.
    part: PathBuf,
    /// Set by any error; later calls return `InvalidData`.
    failed: bool,
}

impl ParquetWriter {
    /// Opens `<path>.part` for writing, truncating a leftover one.
    ///
    /// # Errors
    ///
    /// [`RelationError::InvalidData`] if `header` has no named attributes
    /// (positional, or arity 0: a Parquet file needs one named column per
    /// attribute) or if `path`'s file stem differs from `header.name()`;
    /// [`RelationError::Io`] / [`RelationError::Parquet`] if the file cannot
    /// be created.
    pub fn create<P: AsRef<Path>>(path: P, header: &RelationHeader) -> Result<Self, RelationError> {
        let path = path.as_ref();
        if header.attrs().is_empty() {
            return Err(RelationError::InvalidData(format!(
                "relation `{}` has no named attributes; a Parquet file needs one named column \
                 per attribute",
                header.name()
            )));
        }
        let stem = file_stem(path);
        if stem != header.name() {
            return Err(RelationError::InvalidData(format!(
                "file stem `{stem}` of {} differs from the relation name `{}`",
                path.display(),
                header.name()
            )));
        }
        let schema: SchemaRef = Arc::new(Schema::new(
            header
                .attrs()
                .iter()
                .map(|attr| Field::new(attr, DataType::Int64, false))
                .collect::<Vec<_>>(),
        ));
        let properties = WriterProperties::builder()
            .set_max_row_group_row_count(Some(ROW_GROUP_ROWS))
            .build();
        let mut part_name = path.as_os_str().to_owned();
        part_name.push(".part");
        let part = PathBuf::from(part_name);
        let file = File::create(&part)?;
        let writer = ArrowWriter::try_new(file, schema.clone(), Some(properties)).inspect_err(|_| {
            let _ = std::fs::remove_file(&part);
        })?;
        Ok(ParquetWriter {
            writer: Some(writer),
            schema,
            columns: header
                .attrs()
                .iter()
                .map(|_| Vec::with_capacity(WRITE_CHUNK_ROWS))
                .collect(),
            buffered: 0,
            rows: 0,
            path: path.to_path_buf(),
            part,
            failed: false,
        })
    }

    /// Appends `batch`'s rows.
    ///
    /// # Errors
    ///
    /// [`RelationError::InvalidData`] if the writer already failed, if a
    /// non-empty `batch`'s arity differs from the header's (an empty batch is
    /// a no-op at any arity), if the running total would
    /// exceed [`RowId::MAX`] rows, or if a value exceeds `i64::MAX` (the
    /// message gives its row, counted across all batches, and its column);
    /// [`RelationError::Parquet`] / [`RelationError::Arrow`] if a chunk
    /// cannot be written. The writer is unusable afterwards.
    pub fn write_batch(&mut self, batch: &Tuples) -> Result<(), RelationError> {
        let result = self.append(batch);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Flushes the last chunk, closes the file, renames `<path>.part` to
    /// `path`, and returns the number of rows written.
    ///
    /// # Errors
    ///
    /// [`RelationError::InvalidData`] if the writer already failed;
    /// [`RelationError::Parquet`] / [`RelationError::Arrow`] /
    /// [`RelationError::Io`] if the flush, the close or the rename fails.
    /// On any error `<path>.part` is removed and `path` is not written.
    pub fn finish(mut self) -> Result<u64, RelationError> {
        let result = self.commit();
        if result.is_err() {
            drop(self.writer.take());
            let _ = std::fs::remove_file(&self.part);
        }
        result
    }

    fn append(&mut self, batch: &Tuples) -> Result<(), RelationError> {
        if self.failed {
            return Err(RelationError::InvalidData(format!(
                "the writer for {} failed earlier and accepts no more batches",
                self.path.display()
            )));
        }
        // An empty batch is a no-op whatever its arity, as an empty batch is
        // accepted under any header elsewhere (`Relation::from_tuples`).
        if batch.is_empty() {
            return Ok(());
        }
        if batch.arity() != self.columns.len() {
            return Err(RelationError::InvalidData(format!(
                "a batch of arity {} for a relation of arity {}",
                batch.arity(),
                self.columns.len()
            )));
        }
        if self.rows + batch.len() as u64 > u64::from(RowId::MAX) {
            return Err(RelationError::InvalidData(format!(
                "{} rows exceed the {} rows a relation holds",
                self.rows + batch.len() as u64,
                RowId::MAX
            )));
        }
        for row in batch.rows() {
            let row_number = self.rows;
            for (column_index, (column, &value)) in self.columns.iter_mut().zip(row).enumerate() {
                let value = i64::try_from(value).map_err(|_| {
                    RelationError::InvalidData(format!(
                        "row {row_number}, column {column_index}: {value} exceeds i64::MAX"
                    ))
                })?;
                column.push(value);
            }
            self.rows += 1;
            self.buffered += 1;
            if self.buffered == WRITE_CHUNK_ROWS {
                self.flush_chunk()?;
            }
        }
        Ok(())
    }

    /// Hands the buffered rows, if any, to `ArrowWriter` as one record batch
    /// and starts empty chunk buffers.
    fn flush_chunk(&mut self) -> Result<(), RelationError> {
        if self.buffered == 0 {
            return Ok(());
        }
        let arrays: Vec<ArrayRef> = self
            .columns
            .iter_mut()
            .map(|column| {
                let values = std::mem::replace(column, Vec::with_capacity(WRITE_CHUNK_ROWS));
                Arc::new(Int64Array::from(values)) as ArrayRef
            })
            .collect();
        self.buffered = 0;
        let batch = RecordBatch::try_new(self.schema.clone(), arrays)?;
        self.writer
            .as_mut()
            .expect("a ParquetWriter holds its writer until finish or drop")
            .write(&batch)?;
        Ok(())
    }

    fn commit(&mut self) -> Result<u64, RelationError> {
        if self.failed {
            return Err(RelationError::InvalidData(format!(
                "the writer for {} failed earlier and cannot finish",
                self.path.display()
            )));
        }
        self.flush_chunk()?;
        let writer = self
            .writer
            .take()
            .expect("a ParquetWriter holds its writer until finish or drop");
        writer.close()?;
        std::fs::rename(&self.part, &self.path)?;
        Ok(self.rows)
    }
}

impl Drop for ParquetWriter {
    /// A writer dropped before `finish` removes its `.part` file, ignoring
    /// errors.
    fn drop(&mut self) {
        if self.writer.take().is_some() {
            let _ = std::fs::remove_file(&self.part);
        }
    }
}

/// Writes `tuples` to `path` as a Parquet file: [`ParquetWriter::create`],
/// one [`ParquetWriter::write_batch`], then [`ParquetWriter::finish`]. See
/// [`ParquetWriter`] for the guarantees.
///
/// # Errors
///
/// Those of the three calls.
pub fn write_parquet<P: AsRef<Path>>(
    path: P, header: &RelationHeader, tuples: &Tuples,
) -> Result<(), RelationError> {
    let mut writer = ParquetWriter::create(path, header)?;
    writer.write_batch(tuples)?;
    writer.finish().map(|_| ())
}

```

(The file then continues with the `#[cfg(test)] mod tests` of Step 1, unchanged.)

In `kermit-ds/src/relation.rs`, let the new module use the stem helper.

Current (line 428):

```rust
fn file_stem(path: &Path) -> String {
```

Replace with:

```rust
pub(crate) fn file_stem(path: &Path) -> String {
```

In `kermit-ds/src/lib.rs`, re-export the two items beside `read_parquet`.

Current (lines 51-52):

```rust
    morsel::Threads,
    relation::{
```

Replace with:

```rust
    morsel::Threads,
    parquet_writer::{write_parquet, ParquetWriter},
    relation::{
```

- [ ] **Step 4: Run it to see it pass**

```bash
nix develop --command cargo fmt --all
nix develop --command cargo fmt --all --check
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds parquet
nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-ds --all-targets -- -D warnings
nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-ds --no-deps
```

`cargo fmt --all` may re-wrap comments and the longer lines; accept what it writes, then `--check` is silent. The test run lists the seven `parquet_writer::tests` tests (`round_trips_arities_1_2_3`, `round_trips_zero_rows`, `rejects_positional_header`, `rejects_stem_mismatch`, `rejects_batch_arity_mismatch`, `empty_batch_of_any_arity_is_a_no_op`, `rejects_value_above_i64_max`) and every existing `parquet` test in `kermit-ds`, all passing, with `0 failed`. Clippy and `cargo doc` print no warning.

- [ ] **Step 5: Commit**

```bash
git add -N kermit-ds/src/parquet_writer.rs
git commit -m "feat(kermit-ds): add ParquetWriter and write_parquet, the inverse of read_parquet

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-ds/src/parquet_writer.rs kermit-ds/src/lib.rs kermit-ds/src/relation.rs
```

### Task 1.3: Batching, row groups, dropping, failure

The behaviours that a round-trip test cannot see: byte-identical files under every batching, a row-group boundary, a dropped writer leaving no file, and a failed writer refusing more. The code for all four is already in Task 1.2 (they are consequences of its design), so these tests pass on first run. What proves each can fail is a mutation check per behaviour in Step 3.

**Files:**
- Modify: `kermit-ds/src/parquet_writer.rs` (the `mod tests` block; no production code)
- Test: `kermit-ds/src/parquet_writer.rs`

- [ ] **Step 1: Write the failing test**

In `kermit-ds/src/parquet_writer.rs`, change the test module's imports (this file is created in Task 1.2).

Current (`kermit-ds/src/parquet_writer.rs`):

```rust
    use {super::*, crate::read_parquet};
```

Replace with:

```rust
    use {
        super::*,
        crate::read_parquet,
        parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder,
    };
```

Add this helper after `invalid_message`:

```rust
    /// `tuples` cut into batches of `size` rows (the last may be shorter).
    fn batches(tuples: &Tuples, size: usize) -> Vec<Tuples> {
        let arity = tuples.arity();
        tuples
            .as_flat()
            .chunks(size * arity)
            .map(|chunk| Tuples::from_flat(arity, chunk.len() / arity, chunk.to_vec()))
            .collect()
    }
```

Add these tests at the end of the module:

```rust
    /// The same rows give the same file whatever the batching: one-shot, and
    /// batches below, at and across the chunk size.
    #[test]
    fn batching_does_not_change_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let header = header_of("rel", 2);
        let tuples = rows_of(2, 2 * WRITE_CHUNK_ROWS + 3);
        let reference = dir.path().join("rel.parquet");
        write_parquet(&reference, &header, &tuples).unwrap();
        let want = std::fs::read(&reference).unwrap();
        assert_eq!(read_parquet(&reference).unwrap(), (header.clone(), tuples.clone()));
        for size in [1, 7, 1000, WRITE_CHUNK_ROWS - 1, WRITE_CHUNK_ROWS + 1] {
            let subdir = dir.path().join(format!("batch{size}"));
            std::fs::create_dir(&subdir).unwrap();
            let path = subdir.join("rel.parquet");
            let mut writer = ParquetWriter::create(&path, &header).unwrap();
            for batch in batches(&tuples, size) {
                writer.write_batch(&batch).unwrap();
            }
            assert_eq!(writer.finish().unwrap(), tuples.len() as u64, "batch size {size}");
            assert!(
                std::fs::read(&path).unwrap() == want,
                "batch size {size}: bytes differ from the one-shot file"
            );
        }
    }

    /// One row group holds `ROW_GROUP_ROWS` rows, so `ROW_GROUP_ROWS + 3`
    /// rows make two, the second holding the 3.
    #[test]
    fn crosses_a_row_group_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let header = header_of("big", 1);
        let tuples = rows_of(1, ROW_GROUP_ROWS + 3);
        let path = dir.path().join("big.parquet");
        write_parquet(&path, &header, &tuples).unwrap();
        let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(&path).unwrap()).unwrap();
        let metadata = builder.metadata();
        assert_eq!(metadata.num_row_groups(), 2);
        assert_eq!(metadata.row_group(0).num_rows(), ROW_GROUP_ROWS as i64);
        assert_eq!(metadata.row_group(1).num_rows(), 3);
        assert_eq!(read_parquet(&path).unwrap(), (header, tuples));
    }

    #[test]
    fn dropped_writer_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        let part = dir.path().join("edge.parquet.part");
        let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
        // More than one chunk, so `ArrowWriter` has been handed data.
        writer.write_batch(&rows_of(2, WRITE_CHUNK_ROWS + 1)).unwrap();
        assert!(part.exists(), "the .part file exists while the writer lives");
        drop(writer);
        assert!(!part.exists());
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn failed_writer_refuses_more_batches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edge.parquet");
        let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
        writer.write_batch(&rows_of(2, 5)).unwrap();
        // A wrong arity fails the writer ...
        invalid_message(writer.write_batch(&rows_of(3, 1)));
        // ... so even a valid batch is now refused ...
        let message = invalid_message(writer.write_batch(&rows_of(2, 1)));
        assert!(message.contains("failed"), "{message}");
        // ... and so is `finish`, which leaves no file of either name.
        let message = invalid_message(writer.finish());
        assert!(message.contains("failed"), "{message}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
```

- [ ] **Step 2: Run it to see it pass**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds parquet_writer
```

The test build succeeds and all eleven `parquet_writer::tests` tests pass (`test result: ok. 11 passed; 0 failed`). These tests cover behaviour Task 1.2 already implements, so they cannot be seen failing without Step 3's mutants.

- [ ] **Step 3: Mutation checks** (no production code changes are kept)

Four mutants, each applied alone. For each: copy the file aside, apply the edit, confirm the file changed (a mutant that did not apply proves nothing), run the named test and expect `FAILED`, then restore by copying back and confirm with `cmp`. Never restore with `git checkout`.

```bash
SCRATCH=<the executing session's scratchpad>
cp kermit-ds/src/parquet_writer.rs "$SCRATCH/parquet_writer.rs.orig"
```

1. Batching. In `write_batch`, after `let result = self.append(batch);` insert the line `let result = result.and_then(|()| self.flush_chunk());` (each batch now ends a chunk). Run `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds batching_does_not_change_bytes`. It must fail with `batch size 1: bytes differ from the one-shot file` or the first size whose pages cut differently. If it passes, the mutant is not exercised by this data: report that rather than weakening the test.
2. Drop. In `Drop::drop`, change `if self.writer.take().is_some() {` to `if self.writer.take().is_none() {`. Run `... cargo test -p kermit-ds dropped_writer_leaves_no_file`. It must fail at `assert!(!part.exists())`.
3. Row-group size. In `create`, change `Some(ROW_GROUP_ROWS)` to `Some(2 * ROW_GROUP_ROWS)`. Run `... cargo test -p kermit-ds crosses_a_row_group_boundary`. It must fail at `assert_eq!(metadata.num_row_groups(), 2)` (left `1`).
4. Failed flag. In `write_batch`, delete the line `            self.failed = true;`. Run `... cargo test -p kermit-ds failed_writer_refuses_more_batches`. It must fail inside `invalid_message`, with `expected InvalidData, got None`: without the flag the second call succeeds.

After each mutant:

```bash
diff "$SCRATCH/parquet_writer.rs.orig" kermit-ds/src/parquet_writer.rs   # before running: must print the edit
cp "$SCRATCH/parquet_writer.rs.orig" kermit-ds/src/parquet_writer.rs
cmp "$SCRATCH/parquet_writer.rs.orig" kermit-ds/src/parquet_writer.rs    # after restoring: silent
```

- [ ] **Step 4: Run it to see it pass**

```bash
nix develop --command cargo fmt --all
nix develop --command cargo fmt --all --check
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-ds
nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-ds --all-targets -- -D warnings
```

The whole `kermit-ds` suite passes with the original file restored (`cmp` silent), including the eleven `parquet_writer::tests`, and clippy is clean.

- [ ] **Step 5: Commit**

```bash
git commit -m "test(kermit-ds): ParquetWriter bytes are batching-independent and leave no partial file

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-ds/src/parquet_writer.rs
```

## Story 2: The `kermit-generate` crate

- **Developer:** intermediate-dev
- **Files:** `Cargo.toml`, `kermit-generate/Cargo.toml`, `kermit-generate/src/lib.rs`, `kermit-generate/src/output_dir.rs`, `kermit-rdf/src/lib.rs`, `kermit-rdf/src/timestamp.rs`, `kermit/Cargo.toml`
- **Interfaces:** exposes the workspace crate `kermit-generate` (`kermit_generate::{output_dir, OutputDir}` and the crate docs that describe the whole layer) and `kermit_rdf::timestamp::utc_iso8601_now`; uses `kermit_bench::cache::base_cache_dir`. Later stories add the `Generator` layer to this crate's `lib.rs`.
- **Verify:** `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate output_dir`

This story creates the workspace crate that D13 puts the shared generator layer in, and the two small edits the layer needs outside it. Task 2.1 makes kermit-rdf's `utc_iso8601_now` public, which `run`'s `meta.json` timestamp will use (the only edit the design makes to kermit-rdf). Task 2.2 adds the crate (root members and patch table, manifest, `lib.rs` with the crate docs, `#![deny(missing_docs)]`) and `kermit_generate::output_dir`, the pure resolver for `bench gen <kind>`'s `--output-dir` rule (D17). It also adds the `kermit-generate` dependency to the binary's manifest, unused until push 2. The crate docs name every item the layer will have, in plain backticks and without intra-doc links, because the items arrive in Story 3; they also say how this crate's `Generator` relates to `kermit_rdf::generator::Generator` (D16).

### Task 2.1: `utc_iso8601_now` becomes public

**Files:**
- Modify: `kermit-rdf/src/lib.rs:20`
- Modify: `kermit-rdf/src/timestamp.rs:16-19`
- Test: `kermit-rdf/src/timestamp.rs` (a doctest, which compiles as an external user of the crate)

- [ ] **Step 1: Write the failing test.** In `kermit-rdf/src/timestamp.rs`, give `utc_iso8601_now` an example. The doctest is compiled outside the crate, so it fails while the module is private. Replace

````rust
/// Returns the current wall-clock time as a UTC ISO-8601 string of the
/// form `YYYY-MM-DDTHH:MM:SSZ`. Returns the epoch (`1970-01-01T00:00:00Z`)
/// if the system clock is set before the Unix epoch.
pub(crate) fn utc_iso8601_now() -> String {
````

with

````rust
/// Returns the current wall-clock time as a UTC ISO-8601 string of the
/// form `YYYY-MM-DDTHH:MM:SSZ`. Returns the epoch (`1970-01-01T00:00:00Z`)
/// if the system clock is set before the Unix epoch.
///
/// # Example
///
/// ```
/// let now = kermit_rdf::timestamp::utc_iso8601_now();
/// assert_eq!(now.len(), "1970-01-01T00:00:00Z".len());
/// assert!(now.ends_with('Z'));
/// ```
pub(crate) fn utc_iso8601_now() -> String {
````

(The visibility stays `pub(crate)` for this step.)

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test --doc -p kermit-rdf utc_iso8601_now
  ```
  Expected: the doctest fails to compile with ``error[E0603]: module `timestamp` is private``, and the summary reads `test result: FAILED. 0 passed; 1 failed`.

- [ ] **Step 3: Implement.** In `kermit-rdf/src/lib.rs`, line 20 is currently

  ```rust
  mod timestamp;
  ```

  Replace it with

  ```rust
  pub mod timestamp;
  ```

  In `kermit-rdf/src/timestamp.rs`, change the function's visibility (the module's `//!` doc already exists, so `#![deny(missing_docs)]` stays satisfied; `days_since_epoch_to_ymd` stays `pub(crate)`):

  ```rust
  pub fn utc_iso8601_now() -> String {
  ```

  (replacing `pub(crate) fn utc_iso8601_now() -> String {`).

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test --doc -p kermit-rdf utc_iso8601_now
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf timestamp
  nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-rdf --all-targets -- -D warnings
  nix develop --command cargo fmt --all --check
  ```
  Expected: the doctest prints `test src/timestamp.rs - timestamp::utc_iso8601_now (line 22) ... ok` (the line number may differ), the unit tests (`ymd_epoch_zero_is_jan_1_1970`, `ymd_handles_one_year`, `iso_timestamp_well_formed`, ...) pass, clippy is silent and the format check is clean. If `cargo fmt --check` reports a diff, run `nix develop --command cargo fmt --all` and re-check.

- [ ] **Step 5: Commit.**
  ```bash
  git commit -m "feat(rdf): make utc_iso8601_now public for kermit-generate (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-rdf/src/lib.rs kermit-rdf/src/timestamp.rs
  ```

### Task 2.2: The crate, and `output_dir`

**Files:**
- Modify: `Cargo.toml:3-12` (members) and `Cargo.toml:26-33` (the `[patch.crates-io]` table)
- Modify: `kermit/Cargo.toml:26`
- Create: `kermit-generate/Cargo.toml`
- Create: `kermit-generate/src/lib.rs`
- Create: `kermit-generate/src/output_dir.rs`
- Test: `kermit-generate/src/output_dir.rs`

- [ ] **Step 1: Write the failing test.** First the crate around it, then the tests alone.

  In the root `Cargo.toml`, the member list ends

  ```toml
      "kermit-parser",
      "kermit-rdf",
  ]
  ```

  Add the new member after `"kermit-rdf"`:

  ```toml
      "kermit-parser",
      "kermit-rdf",
      "kermit-generate",
  ]
  ```

  and the `[patch.crates-io]` table ends

  ```toml
  kermit-parser = { path = "kermit-parser" }
  kermit-rdf = { path = "kermit-rdf" }
  ```

  Add after the last line:

  ```toml
  kermit-generate = { path = "kermit-generate" }
  ```

  In `kermit/Cargo.toml`, line 26 is

  ```toml
  kermit-rdf = { version = "0.1.1", path = "../kermit-rdf" }
  ```

  Add after it (unused until push 2; an unused dependency is not a warning):

  ```toml
  kermit-generate = { version = "0.1.0", path = "../kermit-generate" }
  ```

  Create `kermit-generate/Cargo.toml`. The `[package]` block is kermit-rdf's; the four internal versions are the ones in those crates' manifests today (`kermit-bench` 0.1.2, `kermit-ds` 0.2.0, `kermit-parser` 0.0.3, `kermit-rdf` 0.1.1):

  ```toml
  [package]
  name = "kermit-generate"
  description = "Non-RDF benchmark generators for Kermit and the layer they share"
  version = "0.1.0"
  authors.workspace = true
  edition.workspace = true
  homepage.workspace = true
  license.workspace = true
  publish.workspace = true
  readme.workspace = true
  repository.workspace = true

  [dependencies]
  kermit-bench = { version = "0.1.2", path = "../kermit-bench" }
  kermit-ds = { version = "0.2.0", path = "../kermit-ds" }
  kermit-parser = { version = "0.0.3", path = "../kermit-parser" }
  kermit-rdf = { version = "0.1.1", path = "../kermit-rdf" }
  anyhow = "1.0"
  serde = { version = "1", features = ["derive"] }
  serde_json = "1"
  serde_yaml = "0.9"

  [dev-dependencies]
  tempfile = "3"
  ```

  Create `kermit-generate/src/lib.rs`. The crate docs are written once, here. They use plain backticks, not intra-doc links, because most of the items they name arrive in Story 3 and none may be linked to a binary item:

  ```rust
  //! Non-RDF benchmark generators for Kermit, and the layer they share.
  //!
  //! `kermit-rdf` turns the output of external RDF tools into relations. The
  //! generators in this crate build their relations themselves (a synthetic
  //! schema, a downloaded graph, a database dump) and write them straight to
  //! Parquet. What every such generator needs is here once, so that each adds
  //! only its own module: `pub mod h2o;`, `pub mod snap;` and `pub mod job;`,
  //! added by the issues that introduce them (#95, #96 and #97).
  //!
  //! The shared layer is:
  //!
  //! - `Generator` and `Generated`: the trait a generator implements. It names
  //!   its output directory, benchmark name and tag, writes its relations
  //!   through a `RelationSink`, and returns the queries and its own
  //!   `meta.json` fields.
  //! - `RelationSink`, `RelationWriter` and `RelationRecord`: write each
  //!   relation as `<name>.parquet`, hash it, and remember its name, columns,
  //!   row count and SHA-256.
  //! - `run`: drives a generator and leaves a cache directory that
  //!   `bench list`, `bench fetch` and `bench run` already understand.
  //! - `GeneratedMeta`: the `meta.json` every generator writes, with the
  //!   generator's own fields flattened beside the common ones.
  //! - `output_dir` and `OutputDir`: where `bench gen <kind>` puts a
  //!   benchmark, as a pure function.
  //!
  //! # Relation to `kermit_rdf::generator::Generator`
  //!
  //! `kermit_rdf::generator::Generator` drives an external RDF tool (the
  //! `WatDiv` binary, the LUBM jar) and partitions the N-Triples it produces.
  //! The `Generator` of this crate has no external tool and no triples: it
  //! writes relations directly. The two share a name and, through
  //! `kermit_rdf::generator::{Target, GeneratorMeta, write_meta_json}`, the
  //! cache layout, but not a trait: `kermit-rdf` cannot host this one,
  //! because it must not depend on `kermit-ds`.
  //!
  //! # The cache contract
  //!
  //! `run` leaves a directory that discovery, spec-drift detection, `--force`
  //! and `bench list` treat like any other generated benchmark. To keep a
  //! failed run from passing for a finished one it does, in order:
  //!
  //! 1. create the output directory and delete any old `meta.json`;
  //! 2. let the generator write its relations. Each is written to
  //!    `<name>.parquet.part` and renamed once complete, so a relation exists
  //!    under its real name only when it is whole;
  //! 3. check the generator's extras and queries;
  //! 4. write `benchmark.yml`, with every relation's SHA-256;
  //! 5. write `meta.json` last. It is the commit marker: a directory without
  //!    it is not a cache, whatever else it holds.
  //!
  //! # The binary
  //!
  //! The binary's `materialize` dispatch calls `run` once per generator kind
  //! and discards the `GeneratedMeta` it returns. `bench gen` may print a
  //! summary from it.
  #![deny(missing_docs)]

  mod output_dir;

  pub use output_dir::{output_dir, OutputDir};
  ```

  Create `kermit-generate/src/output_dir.rs` with the tests alone for now:

  ```rust
  #[cfg(test)]
  mod tests {
      use {super::*, kermit_bench::cache::base_cache_dir};

      #[test]
      fn explicit_parent_is_not_discovered() {
          let parent = tempfile::tempdir().unwrap();
          let out = output_dir("h2o-test", Some(parent.path())).unwrap();
          assert_eq!(
              out,
              OutputDir {
                  path: parent.path().join("h2o-test"),
                  discovered: false,
              }
          );
          assert!(!out.path.exists(), "output_dir must not create the directory");
      }

      #[test]
      fn default_parent_is_the_cache_root() {
          let root = base_cache_dir().unwrap();
          let out = output_dir("h2o-test", None).unwrap();
          assert_eq!(
              out,
              OutputDir {
                  path: root.join("h2o-test"),
                  discovered: true,
              }
          );
      }

      #[test]
      fn explicit_cache_root_is_discovered() {
          let root = base_cache_dir().unwrap();
          let out = output_dir("h2o-test", Some(root.as_path())).unwrap();
          assert_eq!(
              out,
              OutputDir {
                  path: root.join("h2o-test"),
                  discovered: true,
              }
          );
      }
  }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate output_dir
  ```
  Expected: the crate resolves, then fails to compile: ``error[E0432]: unresolved imports `output_dir::output_dir`, `output_dir::OutputDir` `` in `lib.rs`, and ``error[E0425]: cannot find function `output_dir` in this scope`` / ``error[E0422]: cannot find struct, variant or union type `OutputDir` in this scope`` in the tests.

- [ ] **Step 3: Implement.** In `kermit-generate/src/output_dir.rs`, add above the `#[cfg(test)]` block:

  ```rust
  //! Where `bench gen <kind>` writes one benchmark.

  use std::path::{Path, PathBuf};

  /// Where `bench gen <kind>` writes one benchmark.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct OutputDir {
      /// `<parent>/<bench_name>`.
      pub path: PathBuf,
      /// Whether the parent is the cache root that `bench list/fetch/run`
      /// scan, so that the benchmark will be discovered there.
      pub discovered: bool,
  }

  /// Resolves `<parent or the cache root>/<bench_name>`, the rule
  /// `bench gen lubm` applies to its `--output-dir`.
  ///
  /// `parent` is `--output-dir`; `None` means the cache root,
  /// [`kermit_bench::cache::base_cache_dir`]. The result is
  /// [`discovered`](OutputDir::discovered) when the parent is that root.
  /// The comparison is lexical: no symlink or `..` is resolved. The function
  /// is pure: it creates nothing, prints nothing and does not look at the
  /// workspace.
  ///
  /// # Errors
  ///
  /// Fails only when `parent` is `None` and the platform has no cache
  /// directory. With an explicit `parent` it always succeeds; the result is
  /// then undiscovered if there is no cache directory to compare against.
  pub fn output_dir(bench_name: &str, parent: Option<&Path>) -> anyhow::Result<OutputDir> {
      let cache_root = kermit_bench::cache::base_cache_dir();
      let (parent, discovered) = match parent {
          | Some(dir) => (dir.to_path_buf(), cache_root.is_ok_and(|root| root == dir)),
          | None => (
              cache_root.map_err(|e| anyhow::anyhow!("no cache directory available: {e}"))?,
              true,
          ),
      };
      Ok(OutputDir {
          path: parent.join(bench_name),
          discovered,
      })
  }
  ```

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate output_dir
  nix develop --command env CARGO_BUILD_JOBS=2 cargo check -p kermit
  nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-generate -p kermit-rdf --all-targets -- -D warnings
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  nix develop --command cargo fmt --all --check
  ```
  Expected: `test result: ok. 3 passed; 0 failed` (`explicit_parent_is_not_discovered`, `default_parent_is_the_cache_root`, `explicit_cache_root_is_discovered`); `cargo check -p kermit` finishes with the new dependency resolved; clippy, `cargo doc` and the format check are clean. If `cargo fmt --check` reports a diff, run `nix develop --command cargo fmt --all` and re-check.

- [ ] **Step 5: Commit.**
  ```bash
  git add -N kermit-generate/Cargo.toml kermit-generate/src/lib.rs kermit-generate/src/output_dir.rs
  git commit -m "feat(generate): add the kermit-generate crate with output_dir (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- Cargo.toml kermit/Cargo.toml kermit-generate/Cargo.toml kermit-generate/src/lib.rs kermit-generate/src/output_dir.rs
  ```

## Story 3: The layer

- **Developer:** intermediate-dev
- **Files:** `kermit-generate/src/generator.rs`, `kermit-generate/src/sink.rs`, `kermit-generate/src/meta.rs`, `kermit-generate/src/run.rs`, `kermit-generate/src/lib.rs`, `kermit-generate/tests/run.rs`
- **Interfaces:** exposes `kermit_generate::{Generator, Generated, RelationSink, RelationWriter, RelationRecord, GeneratedMeta, run}` (with `output_dir` and `OutputDir` from Story 2); uses `kermit_ds::{ParquetWriter, RelationHeader, Tuples, read_parquet}` (`ParquetWriter` from Story 1), `kermit_bench::{BenchmarkDefinition, QueryDefinition, RelationSource, cache::sha256_hex}`, `kermit_parser::JoinQuery`, `kermit_rdf::generator::{Target, GeneratorMeta, MetaHeader, META_SCHEMA_VERSION, meta_json_path, write_meta_json, read_meta_json}` and `kermit_rdf::timestamp::utc_iso8601_now` (public since Story 2).
- **Verify:** `nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate`

This story is the shared layer itself. Task 3.1 is the sink: `RelationSink` opens a `ParquetWriter` per relation after checking its name (lowercase, so no reserved prefix can pass), `RelationWriter` streams batches and, on `finish`, hashes the file and records it. Task 3.2 is `GeneratedMeta<E>`, the common `meta.json` fields plus a flattened `Extras`, with the list of keys the extras may not use. Task 3.3 is the `Generator` trait and `run`, which enforces the cache contract in the six steps the spec fixes: clear the old `meta.json`, generate, check the extras (D14), check the queries, write `benchmark.yml` with every relation's `sha256` (D9), and write `meta.json` last. Its integration tests drive a configurable toy generator through the public API and cover every failure path the spec names: a failing `generate`, a malformed query, a query over an unwritten relation, extras that collide with a common field, and extras that are not an object. In each, a stale `meta.json` planted beforehand must be gone afterwards. Tasks 3.1 and 3.2 leave `pub(crate)` items that only `run` uses (`RelationSink::new` and `into_records`; `COMMON_FIELDS`), so they carry a temporary `#[allow(dead_code)]` that Task 3.3 removes.

### Task 3.1: `RelationSink`, `RelationWriter` and `RelationRecord`

**Files:**
- Create: `kermit-generate/src/sink.rs`
- Modify: `kermit-generate/src/lib.rs` (the `mod` and `pub use` lines Story 2 wrote)
- Test: `kermit-generate/src/sink.rs`

- [ ] **Step 1: Write the failing test.** In `kermit-generate/src/lib.rs`, Story 2 wrote

  ```rust
  mod output_dir;

  pub use output_dir::{output_dir, OutputDir};
  ```

  Replace it with

  ```rust
  mod output_dir;
  mod sink;

  pub use {
      output_dir::{output_dir, OutputDir},
      sink::{RelationRecord, RelationSink, RelationWriter},
  };
  ```

  Create `kermit-generate/src/sink.rs` with the tests alone for now. They write real Parquet through Story 1's `ParquetWriter`:

  ```rust
  #[cfg(test)]
  mod tests {
      use {super::*, kermit_ds::read_parquet, std::fs};

      fn header(name: &str) -> RelationHeader {
          RelationHeader::new(name, vec!["a".to_string(), "b".to_string()])
      }

      fn pairs() -> Tuples { Tuples::from(vec![vec![1, 2], vec![3, 4]]) }

      #[test]
      fn rejects_uppercase_and_reserved_names() {
          let dir = tempfile::tempdir().unwrap();
          let mut sink = RelationSink::new(dir.path());
          for name in ["Edge", "Const_x", "1x", "x-y", ""] {
              let Err(err) = sink.open(&header(name)) else {
                  panic!("the name {name:?} was accepted");
              };
              assert!(err.to_string().contains("[a-z][a-z0-9_]*"), "{name:?}: {err}");
          }
          assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "a rejected name left a file");
          sink.write(&header("r2_d2"), &pairs()).unwrap();
          assert_eq!(sink.records().len(), 1);
      }

      #[test]
      fn rejects_duplicate_names() {
          let dir = tempfile::tempdir().unwrap();
          let mut sink = RelationSink::new(dir.path());
          sink.write(&header("edge"), &pairs()).unwrap();
          let file = dir.path().join("edge.parquet");
          let before = fs::read(&file).unwrap();
          let Err(err) = sink.open(&header("edge")) else {
              panic!("a second `edge` was accepted");
          };
          assert!(err.to_string().contains("already"), "{err}");
          assert_eq!(sink.records().len(), 1);
          assert_eq!(fs::read(&file).unwrap(), before, "the first file was touched");
      }

      #[test]
      fn record_hash_matches_file() {
          let dir = tempfile::tempdir().unwrap();
          let mut sink = RelationSink::new(dir.path());
          let (edge, rows) = (header("edge"), pairs());
          sink.write(&edge, &rows).unwrap();
          let file = dir.path().join("edge.parquet");
          assert_eq!(
              sink.records(),
              [RelationRecord {
                  name: "edge".to_string(),
                  columns: vec!["a".to_string(), "b".to_string()],
                  rows: 2,
                  sha256: sha256_hex(&file).unwrap(),
              }]
          );
          assert_eq!(read_parquet(&file).unwrap(), (edge, rows));
      }

      #[test]
      fn raw_dir_is_created_on_demand() {
          let dir = tempfile::tempdir().unwrap();
          let sink = RelationSink::new(dir.path());
          let raw = dir.path().join("raw");
          assert!(!raw.exists());
          assert_eq!(sink.raw_dir().unwrap(), raw);
          assert!(raw.is_dir());
          assert_eq!(sink.raw_dir().unwrap(), raw, "a second call must also succeed");
      }

      #[test]
      fn dropped_relation_writer_records_nothing() {
          let dir = tempfile::tempdir().unwrap();
          let mut sink = RelationSink::new(dir.path());
          let mut writer = sink.open(&header("edge")).unwrap();
          writer.write_batch(&pairs()).unwrap();
          drop(writer);
          assert!(sink.records().is_empty());
          let left: Vec<_> = fs::read_dir(dir.path())
              .unwrap()
              .map(|entry| entry.unwrap().file_name())
              .collect();
          assert!(left.is_empty(), "a dropped writer left {left:?}");
          sink.write(&header("edge"), &pairs()).unwrap();
          assert_eq!(sink.records().len(), 1, "the name must be free again");
      }
  }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate sink
  ```
  Expected: compile errors: ``error[E0432]: unresolved import `sink::RelationRecord` `` (and `RelationSink`, `RelationWriter`) in `lib.rs`, and ``error[E0433]: failed to resolve: use of undeclared type `RelationSink` `` in the tests.

- [ ] **Step 3: Implement.** In `kermit-generate/src/sink.rs`, add above the `#[cfg(test)]` block:

  ```rust
  //! Writing a generator's relations: [`RelationSink`] and its records.

  use {
      anyhow::{bail, Context},
      kermit_bench::cache::sha256_hex,
      kermit_ds::{ParquetWriter, RelationHeader, Tuples},
      serde::{Deserialize, Serialize},
      std::path::{Path, PathBuf},
  };

  /// What the sink recorded about one finished relation file. `benchmark.yml`
  /// and `meta.json` are both built from these.
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct RelationRecord {
      /// The relation's name, which is also its file stem.
      pub name: String,
      /// The attribute names, in column order.
      pub columns: Vec<String>,
      /// The number of rows written.
      pub rows: u64,
      /// The SHA-256 of `<name>.parquet`, in the encoding `bench fetch`
      /// compares against: 64 lowercase hex characters.
      pub sha256: String,
  }

  /// Where a generator writes its relations: `<out_dir>/<name>.parquet`,
  /// one file per relation.
  ///
  /// Each file is written by `kermit_ds::ParquetWriter`, so it appears under
  /// its real name only when complete, and its bytes do not depend on how the
  /// generator batched its rows. The sink hashes every finished file and keeps
  /// a [`RelationRecord`] of it, in write order.
  #[derive(Debug)]
  pub struct RelationSink {
      out_dir: PathBuf,
      records: Vec<RelationRecord>,
  }

  impl RelationSink {
      /// An empty sink over `out_dir`, which must exist.
      // Used by `run`, which Task 3.3 adds; Task 3.3 removes this allow.
      #[allow(dead_code)]
      pub(crate) fn new(out_dir: &Path) -> Self {
          Self {
              out_dir: out_dir.to_path_buf(),
              records: Vec::new(),
          }
      }

      /// Starts the relation `header` names, to be filled batch by batch.
      ///
      /// Only one relation can be open at a time: the writer borrows the sink.
      ///
      /// # Errors
      ///
      /// Fails, before creating any file, if the name does not match
      /// `[a-z][a-z0-9_]*` or a relation of that name was already written. The
      /// lowercase rule is stricter than the Datalog parser's identifiers, and
      /// so excludes the reserved prefixes (`Const_`, `Select_`, `Index_`)
      /// too. It also fails if `ParquetWriter::create` does, e.g. for a
      /// positional header.
      pub fn open(&mut self, header: &RelationHeader) -> anyhow::Result<RelationWriter<'_>> {
          let name = header.name();
          check_relation_name(name)?;
          if self.records.iter().any(|record| record.name == name) {
              bail!("the relation {name:?} was already written");
          }
          let path = self.out_dir.join(format!("{name}.parquet"));
          let writer = ParquetWriter::create(&path, header)?;
          Ok(RelationWriter {
              sink: self,
              writer,
              header: header.clone(),
              path,
          })
      }

      /// Writes a whole relation: [`open`](Self::open), one batch, and
      /// [`finish`](RelationWriter::finish).
      pub fn write(&mut self, header: &RelationHeader, tuples: &Tuples) -> anyhow::Result<()> {
          let mut writer = self.open(header)?;
          writer.write_batch(tuples)?;
          writer.finish()
      }

      /// `<out_dir>/raw/`, for a generator's raw artifacts (a downloaded
      /// file, say). Created on the first call.
      pub fn raw_dir(&self) -> anyhow::Result<PathBuf> {
          let dir = self.out_dir.join("raw");
          std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
          Ok(dir)
      }

      /// The relations finished so far, in write order.
      pub fn records(&self) -> &[RelationRecord] { &self.records }

      /// The records, once the generator is done.
      // Used by `run`, which Task 3.3 adds; Task 3.3 removes this allow.
      #[allow(dead_code)]
      pub(crate) fn into_records(self) -> Vec<RelationRecord> { self.records }
  }

  /// A relation being written batch by batch; see [`RelationSink::open`].
  ///
  /// A writer dropped without [`finish`](Self::finish) records nothing and
  /// leaves no file behind.
  pub struct RelationWriter<'a> {
      sink: &'a mut RelationSink,
      writer: ParquetWriter,
      header: RelationHeader,
      path: PathBuf,
  }

  impl RelationWriter<'_> {
      /// Appends `batch`'s rows. Batch boundaries do not show in the file,
      /// and an empty batch is a no-op at any arity.
      ///
      /// # Errors
      ///
      /// Fails as `ParquetWriter::write_batch` does: on a non-empty batch
      /// whose arity is not the header's, a value above `i64::MAX`, or more
      /// than `RowId::MAX` rows in total. After an error the writer refuses
      /// further batches.
      pub fn write_batch(&mut self, batch: &Tuples) -> anyhow::Result<()> {
          Ok(self.writer.write_batch(batch)?)
      }

      /// Finishes the file, hashes it and records it in the sink.
      pub fn finish(self) -> anyhow::Result<()> {
          let rows = self.writer.finish()?;
          let sha256 = sha256_hex(&self.path)?;
          self.sink.records.push(RelationRecord {
              name: self.header.name().to_string(),
              columns: self.header.attrs().to_vec(),
              rows,
              sha256,
          });
          Ok(())
      }
  }

  /// Checks `name` against `[a-z][a-z0-9_]*`, by hand: the crate has no regex
  /// dependency.
  fn check_relation_name(name: &str) -> anyhow::Result<()> {
      let mut chars = name.chars();
      let well_formed = chars.next().is_some_and(|c| c.is_ascii_lowercase())
          && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
      if !well_formed {
          bail!(
              "the relation name {name:?} must match [a-z][a-z0-9_]*, which the Datalog parser \
               accepts and no reserved prefix (`Const_`, `Select_`, `Index_`) starts"
          );
      }
      Ok(())
  }
  ```

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate sink
  nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-generate --all-targets -- -D warnings
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  nix develop --command cargo fmt --all --check
  ```
  Expected: `test result: ok. 5 passed; 0 failed` (`rejects_uppercase_and_reserved_names`, `rejects_duplicate_names`, `record_hash_matches_file`, `raw_dir_is_created_on_demand`, `dropped_relation_writer_records_nothing`); clippy, `cargo doc` and the format check are clean. If `cargo fmt --check` reports a diff, run `nix develop --command cargo fmt --all` and re-check.

- [ ] **Step 5: Commit.**
  ```bash
  git add -N kermit-generate/src/sink.rs
  git commit -m "feat(generate): add RelationSink, RelationWriter and RelationRecord (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/lib.rs kermit-generate/src/sink.rs
  ```

### Task 3.2: `GeneratedMeta`

**Files:**
- Create: `kermit-generate/src/meta.rs`
- Modify: `kermit-generate/src/lib.rs`
- Test: `kermit-generate/src/meta.rs`

- [ ] **Step 1: Write the failing test.** In `kermit-generate/src/lib.rs`, Task 3.1 left

  ```rust
  mod output_dir;
  mod sink;

  pub use {
      output_dir::{output_dir, OutputDir},
      sink::{RelationRecord, RelationSink, RelationWriter},
  };
  ```

  Replace it with

  ```rust
  mod meta;
  mod output_dir;
  mod sink;

  pub use {
      meta::GeneratedMeta,
      output_dir::{output_dir, OutputDir},
      sink::{RelationRecord, RelationSink, RelationWriter},
  };
  ```

  Create `kermit-generate/src/meta.rs` with the tests alone for now:

  ```rust
  #[cfg(test)]
  mod tests {
      use {super::*, std::collections::BTreeSet};

      #[derive(Debug, PartialEq, Serialize, Deserialize)]
      struct Empty {}

      fn meta(spec_hash: Option<&str>) -> GeneratedMeta<Empty> {
          GeneratedMeta {
              schema_version: 4,
              kind: "toy".to_string(),
              tag: "t1".to_string(),
              generated_at_utc: "1970-01-01T00:00:00Z".to_string(),
              relation_count: 2,
              query_count: 1,
              spec_hash: spec_hash.map(str::to_string),
              relations: vec![],
              extras: Empty {},
          }
      }

      #[test]
      fn common_fields_match_serialized_keys() {
          let value = serde_json::to_value(meta(Some("h"))).unwrap();
          let keys: BTreeSet<&str> = value
              .as_object()
              .unwrap()
              .keys()
              .map(String::as_str)
              .collect();
          let listed: BTreeSet<&str> = COMMON_FIELDS.into_iter().collect();
          assert_eq!(keys, listed);
          assert_eq!(listed.len(), COMMON_FIELDS.len(), "COMMON_FIELDS repeats a key");
      }

      #[test]
      fn accessors_return_the_common_fields() {
          let with = meta(Some("h"));
          assert_eq!(with.schema_version(), 4);
          assert_eq!(with.kind(), "toy");
          assert_eq!(with.tag(), "t1");
          assert_eq!(with.generated_at_utc(), "1970-01-01T00:00:00Z");
          assert_eq!(with.relation_count(), 2);
          assert_eq!(with.query_count(), 1);
          assert_eq!(with.spec_hash(), Some("h"));
          assert_eq!(meta(None).spec_hash(), None);
      }
  }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate meta
  ```
  Expected: compile errors: ``error[E0432]: unresolved import `meta::GeneratedMeta` `` in `lib.rs`, and ``error[E0412]: cannot find type `GeneratedMeta` in this scope`` / ``error[E0425]: cannot find value `COMMON_FIELDS` in this scope`` in the tests.

- [ ] **Step 3: Implement.** In `kermit-generate/src/meta.rs`, add above the `#[cfg(test)]` block. The derive needs no `#[serde(bound(...))]`: serde infers `E: Serialize` and `E: Deserialize<'de>` from the flattened field, and the `GeneratorMeta` impl's `E: Serialize + DeserializeOwned` satisfies both.

  ```rust
  //! The `meta.json` every generator writes.

  use {
      crate::sink::RelationRecord,
      kermit_rdf::generator::GeneratorMeta,
      serde::{de::DeserializeOwned, Deserialize, Serialize},
  };

  /// A generator's `meta.json`: the fields every kind shares, then the
  /// generator's own `Extras` flattened beside them.
  ///
  /// `kermit_rdf::generator::MetaHeader` reads the shared fields back and
  /// ignores the rest, so spec-drift detection, the outdated-cache floors and
  /// `bench list` treat every kind alike. An `Extras` type must not reuse the
  /// name of a shared field, because flattening would collide; `run` checks.
  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
  pub struct GeneratedMeta<E> {
      /// Always `kermit_rdf::generator::META_SCHEMA_VERSION`.
      pub schema_version: u32,
      /// The generator's `KIND`.
      pub kind: String,
      /// The user-provided tag.
      pub tag: String,
      /// UTC timestamp of generation.
      pub generated_at_utc: String,
      /// The number of relations written.
      pub relation_count: u32,
      /// The number of queries in `benchmark.yml`.
      pub query_count: u32,
      /// The declarative spec's hash; `None` for an imperative run.
      #[serde(default, skip_serializing_if = "Option::is_none")]
      pub spec_hash: Option<String>,
      /// Every relation written, in write order.
      pub relations: Vec<RelationRecord>,
      /// The generator's own fields.
      #[serde(flatten)]
      pub extras: E,
  }

  impl<E: Serialize + DeserializeOwned> GeneratorMeta for GeneratedMeta<E> {
      fn schema_version(&self) -> u32 { self.schema_version }

      fn kind(&self) -> &str { &self.kind }

      fn tag(&self) -> &str { &self.tag }

      fn generated_at_utc(&self) -> &str { &self.generated_at_utc }

      fn relation_count(&self) -> u32 { self.relation_count }

      fn query_count(&self) -> u32 { self.query_count }

      fn spec_hash(&self) -> Option<&str> { self.spec_hash.as_deref() }
  }

  /// The keys `GeneratedMeta` writes itself; `Extras` may use none of them.
  // Used by `run`, which Task 3.3 adds; Task 3.3 removes this allow.
  #[allow(dead_code)]
  pub(crate) const COMMON_FIELDS: [&str; 8] = [
      "schema_version",
      "kind",
      "tag",
      "generated_at_utc",
      "relation_count",
      "query_count",
      "spec_hash",
      "relations",
  ];
  ```

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate meta
  nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-generate --all-targets -- -D warnings
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-generate --no-deps
  nix develop --command cargo fmt --all --check
  ```
  Expected: `test result: ok. 2 passed; 0 failed` (`common_fields_match_serialized_keys`, `accessors_return_the_common_fields`); clippy, `cargo doc` and the format check are clean. If `cargo fmt --check` reports a diff, run `nix develop --command cargo fmt --all` and re-check.

- [ ] **Step 5: Commit.**
  ```bash
  git add -N kermit-generate/src/meta.rs
  git commit -m "feat(generate): add GeneratedMeta (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/lib.rs kermit-generate/src/meta.rs
  ```

### Task 3.3: `Generator`, `Generated` and `run`

**Files:**
- Create: `kermit-generate/src/generator.rs`
- Create: `kermit-generate/src/run.rs`
- Modify: `kermit-generate/src/lib.rs`
- Modify: `kermit-generate/src/sink.rs` (remove the temporary `#[allow(dead_code)]` on `new` and `into_records`)
- Modify: `kermit-generate/src/meta.rs` (remove the one on `COMMON_FIELDS`)
- Test: `kermit-generate/tests/run.rs`, `kermit-generate/src/run.rs`

- [ ] **Step 1: Write the failing test.** Create `kermit-generate/tests/run.rs`. A configurable `ToyGenerator` writes `edge(src, dst)` in one shot and `node(id)` streamed in three batches, with one query over both. The failure tests plant a stale `meta.json` first; it must be gone afterwards, and no `benchmark.yml` may have been written (every failure here happens before step 5). The expected count is 4: of the five edges, only `(4, 1)` leaves a vertex that is not a node.

  ```rust
  use {
      kermit_bench::{
          cache::sha256_hex, discovery::load_all_benchmarks_with_cache, BenchmarkDefinition,
          QueryDefinition,
      },
      kermit_ds::{read_parquet, RelationHeader, Tuples},
      kermit_generate::{run, Generated, GeneratedMeta, Generator, RelationSink},
      kermit_rdf::generator::{
          meta_json_path, read_meta_json, MetaHeader, Target, META_SCHEMA_VERSION,
      },
      serde::{Deserialize, Serialize},
      serde_json::{json, Value},
      std::{
          fs,
          path::{Path, PathBuf},
      },
  };

  const EDGES: [[usize; 2]; 5] = [[1, 2], [1, 3], [2, 3], [3, 1], [4, 1]];
  const NODES: [usize; 6] = [1, 2, 3, 5, 8, 13];

  /// Writes `edge` in one shot and `node` in three batches, with the one
  /// query `Q(X, Y) :- edge(X, Y), node(X).` (4 answers).
  struct ToyGenerator {
      out_dir: PathBuf,
      spec_hash: Option<String>,
      /// `generate` fails after writing `edge`.
      fail: bool,
      /// The query is not a well-formed rule.
      bad_query: bool,
      /// The query names a relation the generator never wrote.
      unknown_relation: bool,
      /// No query at all: every check passes and `validate()` fails at step
      /// 5, so a `meta.json` written before step 6 would be caught.
      no_queries: bool,
      extras: Value,
  }

  impl ToyGenerator {
      fn new(out_dir: PathBuf) -> Self {
          Self {
              out_dir,
              spec_hash: Some("hash-1".to_string()),
              fail: false,
              bad_query: false,
              unknown_relation: false,
              no_queries: false,
              extras: json!({ "seed": 7, "note": "toy" }),
          }
      }
  }

  impl Generator for ToyGenerator {
      const KIND: &'static str = "toy";

      type Extras = Value;

      fn target(&self) -> Target<'_> {
          Target {
              out_dir: &self.out_dir,
              bench_name: "toy-bench",
              tag: "t1",
              spec_hash: self.spec_hash.as_deref(),
          }
      }

      fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<Value>> {
          let edge = RelationHeader::new("edge", vec!["src".to_string(), "dst".to_string()]);
          let edges: Vec<Vec<usize>> = EDGES.iter().map(|row| row.to_vec()).collect();
          sink.write(&edge, &Tuples::from(edges))?;
          if self.fail {
              anyhow::bail!("toy failure");
          }
          let node = RelationHeader::new("node", vec!["id".to_string()]);
          let mut writer = sink.open(&node)?;
          for chunk in NODES.chunks(2) {
              let mut batch = Tuples::new(1);
              for id in chunk {
                  batch.push(&[*id]);
              }
              writer.write_batch(&batch)?;
          }
          writer.finish()?;
          let rule = if self.bad_query {
              "Q(X, Y) :- edge(X, Y"
          } else if self.unknown_relation {
              "Q(X, Y) :- edge(X, Y), ghost(X)."
          } else {
              "Q(X, Y) :- edge(X, Y), node(X)."
          };
          let queries = if self.no_queries {
              Vec::new()
          } else {
              vec![QueryDefinition {
                  name: "q1".to_string(),
                  description: "edges that leave a node".to_string(),
                  query: rule.to_string(),
                  expected: Some(4),
              }]
          };
          Ok(Generated {
              description: "A toy benchmark".to_string(),
              queries,
              extras: self.extras.clone(),
          })
      }
  }

  #[derive(Debug, PartialEq, Serialize, Deserialize)]
  struct ToyExtras {
      seed: u64,
      note: String,
  }

  /// A `meta.json` a previous run might have left behind.
  fn plant_stale_meta(out_dir: &Path) {
      fs::create_dir_all(out_dir).unwrap();
      fs::write(meta_json_path(out_dir), r#"{"schema_version":4,"kind":"toy"}"#).unwrap();
  }

  /// `run` must fail with a message containing `needle`, and leave neither a
  /// `meta.json` (the stale one included) nor a `benchmark.yml`.
  fn assert_fails_without_meta(generator: &ToyGenerator, needle: &str) {
      plant_stale_meta(&generator.out_dir);
      let err = run(generator).expect_err("run should have failed");
      let message = format!("{err:#}");
      assert!(message.contains(needle), "expected {needle:?} in: {message}");
      assert!(!meta_json_path(&generator.out_dir).exists(), "the stale meta.json survived");
      assert!(!generator.out_dir.join("benchmark.yml").exists(), "benchmark.yml was written");
  }

  #[test]
  fn run_writes_a_discoverable_cache() {
      let dir = tempfile::tempdir().unwrap();
      let cache_root = dir.path().join("cache");
      let out = cache_root.join("toy-bench");
      let meta = run(&ToyGenerator::new(out.clone())).unwrap();

      // benchmark.yml parses, validates, and carries each relation's digest.
      let yaml = fs::read_to_string(out.join("benchmark.yml")).unwrap();
      let definition: BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
      definition.validate().unwrap();
      assert_eq!(definition.name, "toy-bench");
      assert_eq!(definition.description, "A toy benchmark");
      assert!(definition.generator.is_none());
      let names: Vec<&str> = definition.relations.iter().map(|r| r.name.as_str()).collect();
      assert_eq!(names, ["edge", "node"]);
      let canonical = out.canonicalize().unwrap();
      for relation in &definition.relations {
          let url = relation.url.as_deref().unwrap();
          let path = Path::new(url.strip_prefix("file://").unwrap());
          assert_eq!(path, canonical.join(format!("{}.parquet", relation.name)));
          assert!(path.exists(), "{url} does not exist");
          assert_eq!(relation.sha256.as_deref(), Some(sha256_hex(path).unwrap().as_str()));
          assert!(relation.path.is_none());
      }
      assert_eq!(definition.queries.len(), 1);
      assert_eq!(definition.queries[0].expected, Some(4));

      // meta.json carries the shared fields and the extras.
      let header = MetaHeader::read(&out).unwrap();
      assert_eq!(header.kind, "toy");
      assert_eq!(header.spec_hash.as_deref(), Some("hash-1"));
      assert_eq!(header.schema_version, META_SCHEMA_VERSION);
      let read: GeneratedMeta<ToyExtras> = read_meta_json(&out).unwrap();
      assert_eq!(
          read.extras,
          ToyExtras {
              seed: 7,
              note: "toy".to_string(),
          }
      );
      assert_eq!(read.tag, "t1");
      assert_eq!(read.generated_at_utc.len(), "1970-01-01T00:00:00Z".len());
      assert_eq!((read.relation_count, read.query_count), (2, 1));
      assert_eq!(read.relations, meta.relations);
      assert_eq!(meta.extras, json!({ "seed": 7, "note": "toy" }));

      // Discovery finds it, in an otherwise empty workspace.
      let workspace = dir.path().join("workspace");
      let found = load_all_benchmarks_with_cache(&workspace, &cache_root).unwrap();
      assert_eq!(found.len(), 1);
      assert_eq!(found[0].name, "toy-bench");
  }

  #[test]
  fn rows_and_relation_count_are_recorded() {
      let dir = tempfile::tempdir().unwrap();
      let out = dir.path().join("toy-bench");
      let meta = run(&ToyGenerator::new(out.clone())).unwrap();

      assert_eq!((meta.relation_count, meta.query_count), (2, 1));
      let summary: Vec<(&str, Vec<&str>, u64)> = meta
          .relations
          .iter()
          .map(|r| (r.name.as_str(), r.columns.iter().map(String::as_str).collect(), r.rows))
          .collect();
      assert_eq!(summary, [("edge", vec!["src", "dst"], 5), ("node", vec!["id"], 6)]);

      // The streamed relation holds its rows in order, whatever the batching.
      let (header, tuples) = read_parquet(out.join("node.parquet")).unwrap();
      assert_eq!(header, RelationHeader::new("node", vec!["id".to_string()]));
      assert_eq!(tuples.to_vecs(), [[1], [2], [3], [5], [8], [13]]);
  }

  #[test]
  fn failed_generate_writes_no_meta() {
      let dir = tempfile::tempdir().unwrap();
      let toy = ToyGenerator {
          fail: true,
          ..ToyGenerator::new(dir.path().join("toy-bench"))
      };
      assert_fails_without_meta(&toy, "toy failure");
  }

  #[test]
  fn bad_query_writes_no_meta() {
      let dir = tempfile::tempdir().unwrap();
      let toy = ToyGenerator {
          bad_query: true,
          ..ToyGenerator::new(dir.path().join("toy-bench"))
      };
      assert_fails_without_meta(&toy, "\"q1\"");
  }

  #[test]
  fn unknown_relation_writes_no_meta() {
      let dir = tempfile::tempdir().unwrap();
      let toy = ToyGenerator {
          unknown_relation: true,
          ..ToyGenerator::new(dir.path().join("toy-bench"))
      };
      assert_fails_without_meta(&toy, "ghost");
  }

  #[test]
  fn colliding_extras_write_no_meta() {
      let dir = tempfile::tempdir().unwrap();
      let toy = ToyGenerator {
          extras: json!({ "relations": 1 }),
          ..ToyGenerator::new(dir.path().join("toy-bench"))
      };
      assert_fails_without_meta(&toy, "\"relations\"");
  }

  #[test]
  fn non_object_extras_write_no_meta() {
      let dir = tempfile::tempdir().unwrap();
      let toy = ToyGenerator {
          extras: json!(42),
          ..ToyGenerator::new(dir.path().join("toy-bench"))
      };
      assert_fails_without_meta(&toy, "JSON object");
  }

  /// `validate()` runs at step 5, after every check passed; `meta.json`
  /// (step 6) must still be absent, which pins that it is written last.
  #[test]
  fn no_queries_write_no_meta() {
      let dir = tempfile::tempdir().unwrap();
      let toy = ToyGenerator {
          no_queries: true,
          ..ToyGenerator::new(dir.path().join("toy-bench"))
      };
      assert_fails_without_meta(&toy, "queries must not be empty");
  }
  ```

- [ ] **Step 2: Run it to see it fail.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate --test run
  ```
  Expected: the test target fails to compile: ``error[E0432]: unresolved imports `kermit_generate::run`, `kermit_generate::Generated`, `kermit_generate::Generator` ``.

- [ ] **Step 3: Implement.**

  First `kermit-generate/src/generator.rs`:

  ```rust
  //! The [`Generator`] trait and what a generator returns.

  use {
      crate::sink::RelationSink,
      kermit_bench::QueryDefinition,
      kermit_rdf::generator::Target,
      serde::{de::DeserializeOwned, Serialize},
  };

  /// A benchmark generator that writes its relations directly.
  ///
  /// Implement it, then hand it to [`run`](fn@crate::run), which creates the
  /// output directory, calls [`generate`](Self::generate) and writes
  /// `benchmark.yml` and `meta.json` around it. Unlike
  /// `kermit_rdf::generator::Generator`, which post-processes the output of
  /// an external RDF tool, this trait has no tool and no triples: the
  /// generator produces tuples and the sink stores them.
  pub trait Generator {
      /// `meta.json`'s `kind` (`"h2o"`, `"snap"`, `"job"`).
      const KIND: &'static str;

      /// This generator's own `meta.json` fields, flattened beside the common
      /// ones. It must serialize to a JSON object that uses none of the common
      /// field names ([`GeneratedMeta`](crate::GeneratedMeta)); `run` checks.
      type Extras: Serialize + DeserializeOwned;

      /// Output directory, benchmark name, tag and spec hash.
      fn target(&self) -> Target<'_>;

      /// Writes every relation through `sink` and returns the queries.
      ///
      /// # Errors
      ///
      /// Any error aborts the run before `benchmark.yml` and `meta.json` are
      /// written.
      fn generate(&self, sink: &mut RelationSink) -> anyhow::Result<Generated<Self::Extras>>;
  }

  /// What [`Generator::generate`] returns besides the relations it wrote.
  #[derive(Debug, Clone)]
  pub struct Generated<E> {
      /// `benchmark.yml`'s `description`.
      pub description: String,
      /// The benchmark's queries, each with the cardinality it must return
      /// when the generator knows it. Every body predicate must name a
      /// relation the generator wrote.
      pub queries: Vec<QueryDefinition>,
      /// The generator's own `meta.json` fields.
      pub extras: E,
  }
  ```

  Then `kermit-generate/src/run.rs`. The unit tests cover what the integration suite cannot reach by name: duplicate query names, every common field refused as an extra, and the query check's accept path.

  ```rust
  //! The sequence every generator goes through.

  use {
      crate::{
          generator::Generator,
          meta::{GeneratedMeta, COMMON_FIELDS},
          sink::{RelationRecord, RelationSink},
      },
      anyhow::{bail, Context},
      kermit_bench::{BenchmarkDefinition, QueryDefinition, RelationSource},
      kermit_parser::JoinQuery,
      kermit_rdf::{
          generator::{meta_json_path, write_meta_json, META_SCHEMA_VERSION},
          timestamp::utc_iso8601_now,
      },
      serde::Serialize,
      serde_json::Value,
      std::{collections::HashSet, fs, io::ErrorKind, path::Path},
  };

  /// Runs `generator` and leaves a complete benchmark cache at its target.
  ///
  /// The steps and their order are the cache contract:
  ///
  /// 1. Create the output directory and delete any existing `meta.json`, so
  ///    that a failed run can never leave an old commit marker beside new
  ///    files.
  /// 2. Call [`Generator::generate`] with a fresh [`RelationSink`].
  /// 3. Check the extras: they must serialize to a JSON object, and none of
  ///    its keys may be a common `meta.json` field.
  /// 4. Check the queries: names are unique, each one parses as a Datalog
  ///    rule, and each body predicate names a relation the generator wrote.
  ///    A malformed rule therefore fails here, not at `bench run`.
  /// 5. Write `benchmark.yml`: one `file://` URL and one `sha256` per
  ///    relation, `generator: None`, validated first. A generator that wrote
  ///    no relation or no query fails that validation.
  /// 6. Write `meta.json` last, and return it.
  ///
  /// # Errors
  ///
  /// Any failing step, with the stale `meta.json` already gone. Files the
  /// generator wrote before the failure stay in the directory; without a
  /// `meta.json` they are not a cache.
  pub fn run<G: Generator>(generator: &G) -> anyhow::Result<GeneratedMeta<G::Extras>> {
      let target = generator.target();
      let out_dir = target.out_dir;

      clear_commit_marker(out_dir)?;

      let mut sink = RelationSink::new(out_dir);
      let generated = generator.generate(&mut sink)?;

      check_extras(&generated.extras)?;
      check_queries(&generated.queries, sink.records())?;

      let relation_count =
          u32::try_from(sink.records().len()).context("more relations than fit a u32")?;
      let query_count =
          u32::try_from(generated.queries.len()).context("more queries than fit a u32")?;
      write_benchmark_yaml(
          out_dir,
          target.bench_name,
          generated.description,
          generated.queries,
          sink.records(),
      )?;

      let meta = GeneratedMeta {
          schema_version: META_SCHEMA_VERSION,
          kind: G::KIND.to_string(),
          tag: target.tag.to_string(),
          generated_at_utc: utc_iso8601_now(),
          relation_count,
          query_count,
          spec_hash: target.spec_hash.map(str::to_string),
          relations: sink.into_records(),
          extras: generated.extras,
      };
      write_meta_json(out_dir, &meta)?;
      Ok(meta)
  }

  /// Step 1: creates `out_dir` and deletes `meta.json` if there is one.
  fn clear_commit_marker(out_dir: &Path) -> anyhow::Result<()> {
      fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
      let marker = meta_json_path(out_dir);
      match fs::remove_file(&marker) {
          | Ok(()) => Ok(()),
          | Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
          | Err(e) => Err(e).with_context(|| format!("removing {}", marker.display())),
      }
  }

  /// Step 3: the extras must be a JSON object that avoids the common fields.
  /// `serde(flatten)` would otherwise let a duplicate key through silently.
  fn check_extras<E: Serialize>(extras: &E) -> anyhow::Result<()> {
      let value = serde_json::to_value(extras).context("serializing the generator's extras")?;
      let fields = match value {
          | Value::Object(fields) => fields,
          | other => bail!("a generator's extras must serialize to a JSON object, not {other}"),
      };
      if let Some(key) = COMMON_FIELDS
          .into_iter()
          .find(|key| fields.contains_key(*key))
      {
          bail!(
              "a generator's extras use the key {key:?}, which meta.json already uses for a \
               common field"
          );
      }
      Ok(())
  }

  /// Step 4: unique names, well-formed rules, and body predicates that name
  /// written relations.
  fn check_queries(queries: &[QueryDefinition], records: &[RelationRecord]) -> anyhow::Result<()> {
      let mut names = HashSet::new();
      for query in queries {
          if !names.insert(query.name.as_str()) {
              bail!("the query name {:?} is used twice", query.name);
          }
          let rule: JoinQuery = query
              .query
              .parse()
              .with_context(|| format!("the query {:?} is not a Datalog rule", query.name))?;
          for predicate in &rule.body {
              if !records.iter().any(|record| record.name == predicate.name) {
                  bail!(
                      "the query {:?} names the relation {:?}, which the generator did not write",
                      query.name,
                      predicate.name
                  );
              }
          }
      }
      Ok(())
  }

  /// Step 5: writes `<out_dir>/benchmark.yml`, a validated
  /// `BenchmarkDefinition` over the finished files. As the RDF pipelines do,
  /// it points at each file by an absolute `file://` URL.
  fn write_benchmark_yaml(
      out_dir: &Path, bench_name: &str, description: String, queries: Vec<QueryDefinition>,
      records: &[RelationRecord],
  ) -> anyhow::Result<()> {
      let base = out_dir
          .canonicalize()
          .with_context(|| format!("resolving {}", out_dir.display()))?;
      let relations = records
          .iter()
          .map(|record| RelationSource {
              name: record.name.clone(),
              url: Some(format!("file://{}/{}.parquet", base.display(), record.name)),
              path: None,
              sha256: Some(record.sha256.clone()),
          })
          .collect();
      let definition = BenchmarkDefinition {
          name: bench_name.to_string(),
          description,
          relations,
          queries,
          generator: None,
      };
      definition.validate()?;
      let path = out_dir.join("benchmark.yml");
      fs::write(&path, serde_yaml::to_string(&definition)?)
          .with_context(|| format!("writing {}", path.display()))
  }

  #[cfg(test)]
  mod tests {
      use {super::*, serde_json::json};

      fn record(name: &str) -> RelationRecord {
          RelationRecord {
              name: name.to_string(),
              columns: vec!["a".to_string()],
              rows: 1,
              sha256: "0".repeat(64),
          }
      }

      fn query(name: &str, rule: &str) -> QueryDefinition {
          QueryDefinition {
              name: name.to_string(),
              description: String::new(),
              query: rule.to_string(),
              expected: None,
          }
      }

      #[test]
      fn duplicate_query_names_are_rejected() {
          let queries = [query("q1", "Q(X) :- edge(X)."), query("q1", "Q(X) :- edge(X).")];
          let err = check_queries(&queries, &[record("edge")]).unwrap_err();
          assert!(err.to_string().contains("\"q1\""), "{err}");
          assert!(err.to_string().contains("twice"), "{err}");
      }

      #[test]
      fn every_body_predicate_must_be_a_written_relation() {
          let records = [record("edge"), record("node")];
          let both = [query("q1", "Q(X) :- edge(X), node(X).")];
          check_queries(&both, &records).unwrap();
          let missing = [query("q2", "Q(X) :- edge(X), vertex(X).")];
          let err = check_queries(&missing, &records).unwrap_err();
          assert!(err.to_string().contains("\"vertex\""), "{err}");
      }

      #[test]
      fn a_malformed_rule_names_its_query() {
          let queries = [query("q9", "Q(X) :- edge(X")];
          let err = check_queries(&queries, &[record("edge")]).unwrap_err();
          assert!(format!("{err:#}").contains("\"q9\""), "{err:#}");
      }

      #[test]
      fn extras_must_be_an_object() {
          check_extras(&json!({ "seed": 1 })).unwrap();
          check_extras(&json!({})).unwrap();
          for not_an_object in [json!(42), json!("text"), json!([1, 2]), Value::Null] {
              let err = check_extras(&not_an_object).unwrap_err();
              assert!(err.to_string().contains("JSON object"), "{err}");
          }
      }

      #[test]
      fn every_common_field_is_refused_as_an_extra() {
          for key in COMMON_FIELDS {
              let mut fields = serde_json::Map::new();
              fields.insert(key.to_string(), json!(1));
              let err = check_extras(&Value::Object(fields)).unwrap_err();
              assert!(err.to_string().contains(&format!("{key:?}")), "{key}: {err}");
          }
      }
  }
  ```

  Then wire `lib.rs`. Task 3.2 left

  ```rust
  mod meta;
  mod output_dir;
  mod sink;

  pub use {
      meta::GeneratedMeta,
      output_dir::{output_dir, OutputDir},
      sink::{RelationRecord, RelationSink, RelationWriter},
  };
  ```

  Replace it with the final form:

  ```rust
  mod generator;
  mod meta;
  mod output_dir;
  mod run;
  mod sink;

  pub use {
      generator::{Generated, Generator},
      meta::GeneratedMeta,
      output_dir::{output_dir, OutputDir},
      run::run,
      sink::{RelationRecord, RelationSink, RelationWriter},
  };
  ```

  Finally remove the temporary allows. In `kermit-generate/src/sink.rs`, replace

  ```rust
      /// An empty sink over `out_dir`, which must exist.
      // Used by `run`, which Task 3.3 adds; Task 3.3 removes this allow.
      #[allow(dead_code)]
      pub(crate) fn new(out_dir: &Path) -> Self {
  ```

  with

  ```rust
      /// An empty sink over `out_dir`, which must exist.
      pub(crate) fn new(out_dir: &Path) -> Self {
  ```

  and replace

  ```rust
      /// The records, once the generator is done.
      // Used by `run`, which Task 3.3 adds; Task 3.3 removes this allow.
      #[allow(dead_code)]
      pub(crate) fn into_records(self) -> Vec<RelationRecord> { self.records }
  ```

  with

  ```rust
      /// The records, once the generator is done.
      pub(crate) fn into_records(self) -> Vec<RelationRecord> { self.records }
  ```

  In `kermit-generate/src/meta.rs`, replace

  ```rust
  /// The keys `GeneratedMeta` writes itself; `Extras` may use none of them.
  // Used by `run`, which Task 3.3 adds; Task 3.3 removes this allow.
  #[allow(dead_code)]
  pub(crate) const COMMON_FIELDS: [&str; 8] = [
  ```

  with

  ```rust
  /// The keys `GeneratedMeta` writes itself; `Extras` may use none of them.
  pub(crate) const COMMON_FIELDS: [&str; 8] = [
  ```

- [ ] **Step 4: Run it to see it pass.**
  ```bash
  nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate
  nix develop --command env CARGO_BUILD_JOBS=2 cargo clippy -p kermit-generate --all-targets -- -D warnings
  nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
  nix develop --command cargo fmt --all --check
  ```
  Expected: one `test result:` line per target: `ok. 15 passed; 0 failed` for the unit tests (3 in `output_dir`, 5 in `sink`, 2 in `meta`, 5 in `run`), `ok. 8 passed; 0 failed` for the integration tests (`run_writes_a_discoverable_cache`, `rows_and_relation_count_are_recorded`, `failed_generate_writes_no_meta`, `bad_query_writes_no_meta`, `unknown_relation_writes_no_meta`, `colliding_extras_write_no_meta`, `non_object_extras_write_no_meta`, `no_queries_write_no_meta`), and the doctests; clippy, `cargo doc` (whole workspace, as CI runs it) and the format check are clean. If `cargo fmt --check` reports a diff, run `nix develop --command cargo fmt --all` and re-check. Then check that the tests can fail, with two mutants of `kermit-generate/src/run.rs`. Copy the file aside first (`cp kermit-generate/src/run.rs "$SCRATCH/run.rs.orig"`), restore it after each mutant with `cp "$SCRATCH/run.rs.orig" kermit-generate/src/run.rs`, and check the restore with `cmp` (silent). Never use `git checkout`: the task's files are uncommitted.
  1. Replace the line `clear_commit_marker(out_dir)?;` with `fs::create_dir_all(out_dir)?;`, so only the deletion of the old marker goes. Re-run `cargo test -p kermit-generate --test run`: the six failure tests (`failed_generate_writes_no_meta` through `no_queries_write_no_meta`) must fail with `the stale meta.json survived`, and the two success tests must pass.
  2. Insert the line `fs::write(meta_json_path(out_dir), "{}")?;` immediately above `write_benchmark_yaml(` in `run`, a commit marker written before step 5. Re-run the same command: `no_queries_write_no_meta` must fail with `the stale meta.json survived` (`validate()` fails inside `write_benchmark_yaml`, after the early marker).
  Restore, `cmp`, then re-run the full suite.

- [ ] **Step 5: Commit.**
  ```bash
  git add -N kermit-generate/src/generator.rs kermit-generate/src/run.rs kermit-generate/tests/run.rs
  git commit -m "feat(generate): add Generator and run with the cache contract (#95)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/lib.rs kermit-generate/src/sink.rs kermit-generate/src/meta.rs kermit-generate/src/generator.rs kermit-generate/src/run.rs kermit-generate/tests/run.rs
  ```

## Story 4: CLAUDE.md and ARCHITECTURE.md

- **Developer:** junior-dev
- **Files:** `CLAUDE.md`, `ARCHITECTURE.md`
- **Interfaces:** documents the `kermit-generate` crate from Stories 1–3; no code
- **Verify:** `grep -cF 'workspace with 9 crates' CLAUDE.md`

Exact before/after Markdown edits in two files; no code, no cargo.

### Task 4.1: CLAUDE.md and ARCHITECTURE.md

The workspace gains its ninth crate, so both top-level docs that list the crates and their
dependency graph change. Push 1 adds no `bench gen` kind and no binary code, so the
`kermit` binary's row, "Adding a new benchmark" and the generator-kind lists stay as they
are; push 2 (#95's H2O) edits them.

Each edit below is an exact before/after, with line numbers at `BASE`. No earlier task
edits a Markdown file, so the numbers hold. Before each edit, check that the "before" is
unique with `grep -cF '<before>' <file>` (expect `1`).

**Files:**
- Modify: `CLAUDE.md` (7, after 118, 132, 134, after 331)
- Modify: `ARCHITECTURE.md` (after 24, 35–36, after 37, after 52, before 429)

- [ ] **Step 1: CLAUDE.md, the crate count** (line 7). Before:
  ```markdown
  It is a Cargo workspace with 8 crates.
  ```
  After:
  ```markdown
  It is a Cargo workspace with 9 crates.
  ```

- [ ] **Step 2: CLAUDE.md, the Workspace Architecture row.** Insert after line 118 (`and
  are exposed via `lubm::queries::lubm_query_specs`.`) and before line 119 (`kermit
  → CLI binary (clap)…`). The label column is 16 characters wide, as in the other rows:
  ```text
  kermit-generate → Non-RDF benchmark generators and the layer they share (#95, #96, #97).
                    A `Generator` (`KIND`, `Extras`, `target`, `generate`) writes each
                    relation through a `RelationSink`: one-shot `write`, or `open` →
                    `RelationWriter::write_batch` → `finish`, over kermit-ds's
                    `ParquetWriter` (`<name>.parquet.part`, renamed on finish; bytes
                    independent of batching). `run` deletes the old `meta.json`, checks the
                    extras and the queries, writes `benchmark.yml` (`file://` URLs and each
                    relation's sha256) and then `meta.json` (`GeneratedMeta<E>`, which
                    implements kermit-rdf's `GeneratorMeta`), so discovery, drift and
                    `bench list` treat its caches like kermit-rdf's. `output_dir` resolves
                    `bench gen`'s `--output-dir`. Generators land as `pub mod <kind>`:
                    `h2o` (#95), `snap` (#96), `job` (#97).
  ```

- [ ] **Step 3: CLAUDE.md, the dependency flow** (line 132). Two substrings. Before:
  ```markdown
  `kermit-bench` → `kermit-rdf`; everything → `kermit` (binary).
  ```
  After:
  ```markdown
  `kermit-bench` → `kermit-rdf`; `kermit-ds`/`kermit-bench`/`kermit-rdf`/`kermit-parser` → `kermit-generate`; everything → `kermit` (binary).
  ```
  Before:
  ```markdown
  `kermit-parser` is consumed by `kermit-algos` only.
  ```
  After:
  ```markdown
  `kermit-parser` is consumed by `kermit-algos` and `kermit-generate` only.
  ```

- [ ] **Step 4: CLAUDE.md, the kermit-rdf blockquote** (line 134). Before:
  ```markdown
  if you find yourself wanting one, the pipeline boundary has probably moved.
  ```
  After:
  ```markdown
  if you find yourself wanting one, the pipeline boundary has probably moved. A generator that writes relations itself (and so needs `kermit-ds`) belongs in `kermit-generate`, not `kermit-rdf`.
  ```

- [ ] **Step 5: CLAUDE.md, a gotcha.** Insert a new bullet after line 331 (the bullet that
  opens `- **Cache-side `benchmark.yml` strips `generator:`**`):
  ```markdown
  - **Non-RDF generators live in `kermit-generate`** (#95): kermit-rdf must not depend on kermit-ds, so a generator that writes its relations itself (H2O, SNAP, JOB) implements `kermit_generate::Generator`, not `kermit_rdf::generator::Generator`, which drives external RDF tools and partitions their N-Triples. A generator's own dependencies (`flate2`, `sqlparser`, …) go in `kermit-generate/Cargo.toml`, never `kermit/Cargo.toml`. `kermit_generate::run` writes each relation as `<name>.parquet.part`, renamed on `finish`, and `meta.json` last as the commit marker, after deleting any old one; `ParquetWriter` (kermit-ds) re-chunks rows into fixed 2^16-row chunks, because parquet-rs cuts data pages at checkpoints counted from each input batch, so a file's bytes do not depend on how the caller batched them. `run` rejects `Extras` keys that collide with `GeneratedMeta`'s common fields (`serde(flatten)` would let them through), and `RelationSink` takes only names matching `[a-z][a-z0-9_]*`, which excludes the reserved `Const_`/`Select_`/`Index_` prefixes.
  ```

- [ ] **Step 6: ARCHITECTURE.md, the workspace tree.** Insert after line 24 (`├──
  kermit-rdf/      # RDF/SPARQL pipelines (WatDiv, LUBM generators)`):
  ```text
  ├── kermit-generate/ # Non-RDF generators (H2O, SNAP, JOB) and their shared layer
  ```

- [ ] **Step 7: ARCHITECTURE.md, the dependency graph** (lines 35–37). Before:
  ```text
  kermit         ──▶ kermit-iters, kermit-ds, kermit-algos,
                     kermit-parser, kermit-bench, kermit-rdf
  kermit-rdf     ──▶ kermit-bench
  ```
  After:
  ```text
  kermit         ──▶ kermit-iters, kermit-ds, kermit-algos,
                     kermit-parser, kermit-bench, kermit-rdf,
                     kermit-generate
  kermit-generate ─▶ kermit-ds, kermit-bench, kermit-rdf, kermit-parser
  kermit-rdf     ──▶ kermit-bench
  ```

- [ ] **Step 8: ARCHITECTURE.md, the dependency prose.** Insert a paragraph after line 52
  (the one opening `` `kermit-rdf` depends on `kermit-bench` alone.``), separated by a blank
  line:
  ```markdown
  `kermit-generate` holds the generators that write their relations themselves rather than partitioning an RDF tool's N-Triples: H2O (#95), SNAP (#96) and JOB (#97). Writing relations needs `kermit-ds`'s `ParquetWriter`, which is exactly the dependency `kermit-rdf` must not take, so they get their own crate. It reuses `kermit-rdf`'s cache contract (`Target`, `GeneratorMeta`, `write_meta_json`, `META_SCHEMA_VERSION`) and `kermit-parser` to check each query it emits. Nothing but the binary depends on it.
  ```

- [ ] **Step 9: ARCHITECTURE.md, a section.** Insert before line 429 (`## CLI (`kermit`)`),
  followed by a blank line:
  ```markdown
  ## Non-RDF Generators (`kermit-generate`)

  `kermit-generate` is the layer that non-RDF generator benchmarks share, plus those generators. A generator implements `Generator`: a `KIND` for `meta.json`, an `Extras` type for its own `meta.json` fields, a `target()` (output directory, benchmark name, tag, spec hash: `kermit-rdf`'s `Target`), and `generate(&mut RelationSink)`, which writes every relation and returns the queries with their `expected` counts.

  - **`RelationSink`** writes `<out_dir>/<name>.parquet` through `kermit_ds::ParquetWriter`, either one-shot (`write(&header, &tuples)`) or streamed (`open(&header)` → `RelationWriter::write_batch` → `finish`), and records each relation's columns, rows and SHA-256. A writer that is dropped unfinished leaves no file and no record. `raw_dir()` gives `<out_dir>/raw/` for downloaded artifacts.
  - **`run`** owns the cache contract that `materialize` and discovery rely on. It deletes any old `meta.json`, calls `generate`, checks the extras and that every query parses and names a written relation, writes `benchmark.yml`, and writes `meta.json` last. The last write is the commit marker: a directory without `meta.json` is never a cache hit.
  - **`GeneratedMeta<E>`** is the common `meta.json` (schema version, kind, tag, timestamp, counts, spec hash, per-relation records) with `E` flattened beside it. It implements `kermit-rdf`'s `GeneratorMeta`, so drift detection, `OUTDATED_BEFORE` and `bench list` treat these caches like `kermit-rdf`'s.
  - **`output_dir`** resolves a `bench gen <kind>` output directory (`--output-dir` or the cache root) and reports whether discovery will see it.
  ```

- [ ] **Step 10: Check.** These are Markdown edits, so the check is textual:
  ```bash
  grep -o 'kermit-generate' CLAUDE.md | wc -l        # expect 6
  grep -o 'kermit-generate' ARCHITECTURE.md | wc -l  # expect 6
  grep -cF 'workspace with 9 crates' CLAUDE.md  # expect 1
  ```
  Read both files' edited regions once in full to confirm they render: the code fence in
  the Workspace Architecture block stays closed, and the dependency graph still aligns.

- [ ] **Step 11: Commit.**
  ```bash
  git commit -F - -- CLAUDE.md ARCHITECTURE.md <<'EOF'
  docs: kermit-generate, the shared non-RDF generator layer (#95)

  CLAUDE.md and ARCHITECTURE.md list the ninth crate, its dependency
  edges, its cache contract and why kermit-rdf cannot host it.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD
  EOF
  ```

## Final gate

Run after Story 4, in the slot the co-ordinator allots after `FULL_EXIT`. Check the host
first (Ground rules) before any of these commands.

- [ ] **Step 1: Scope.** The diff against `BASE` touches only the File map's files:
  ```bash
  git diff --name-only BASE..HEAD | sort
  ```
  Expected, exactly: `ARCHITECTURE.md`, `CLAUDE.md`, `Cargo.toml`, `kermit-ds/src/lib.rs`,
  `kermit-ds/src/parquet_writer.rs`, `kermit-ds/src/relation.rs`,
  `kermit-generate/Cargo.toml`, `kermit-generate/src/generator.rs`,
  `kermit-generate/src/lib.rs`, `kermit-generate/src/meta.rs`,
  `kermit-generate/src/output_dir.rs`, `kermit-generate/src/run.rs`,
  `kermit-generate/src/sink.rs`, `kermit-generate/tests/run.rs`, `kermit-rdf/src/lib.rs`,
  `kermit-rdf/src/timestamp.rs` and `kermit/Cargo.toml`. Nothing under `.superpowers/`,
  `kermit/src/` or any data-structure or algorithm module.
- [ ] **Step 2: Format.** `nix develop --command cargo fmt --all --check` prints nothing.
- [ ] **Step 3: Clippy.** `nix develop --command env CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets` ends with no warning or error. Its non-test pass is what would catch dead code in a library crate's private items.
- [ ] **Step 4: Docs.** `nix develop --command env CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps` succeeds; it lints `kermit-generate`'s crate docs.
- [ ] **Step 5: The whole workspace's tests**, detached:
  ```bash
  setsid nohup nix develop --command env CARGO_BUILD_JOBS=2 RUST_BACKTRACE=1 cargo test --workspace > "$SCRATCH/gate-test.log" 2>&1 & disown
  ```
  Watch the PID with `kill -0` until it exits. Then
  `grep -E '^test result:|FAILED|panicked' "$SCRATCH/gate-test.log"` shows only
  `test result: ok.` lines. Expected new counts: `kermit-ds` gains the eleven
  `parquet_writer::tests` tests; `kermit-generate` runs 15 unit tests, 8 integration tests
  and its doctests; `kermit-rdf` gains the `utc_iso8601_now` doctest.
- [ ] **Step 6: Report** the six results to the design session (≤ 15 lines), with the
  final `HEAD`.

## After this plan: landing push 1

Not part of execution. The design session does these, in order:

1. Tell the user that push 1 is gated, and ask for the go-ahead to land it. Pushing to
   master is outward-facing, and approval for this one push is not standing approval.
2. On a yes, land it the loom way:
   - `git fetch origin`;
   - `git merge origin/master` (merge, never rebase);
   - if the merge brought anything in, re-run the final gate's Steps 2–5 on the merged tree;
   - `git push origin HEAD:master`, with no `--force`.
3. Send the pushed SHA to the benchmark co-ordinator session, which relays it, and directly
   to the snap (#96) and job (#97) sessions, as promised. Their generator work starts from
   it.
4. Update the #95 project memory.
5. Push 2 (#95's H2O generator) gets its own plan. It must not start before push 1 is on
   master.

## Merge notes

The drafts were written in parallel against the names contract (S1 by one drafter, S2 and
S3 by another, S4 by the design session). Choices made while merging:

- **The row-group setter.** `WriterPropertiesBuilder::set_max_row_group_size` is
  deprecated since parquet 58.0.0 (`parquet-59.3.0/src/file/properties.rs:741`) and would
  fail clippy. Story 1 uses `set_max_row_group_row_count(Some(ROW_GROUP_ROWS))`, and the
  contract records it (0de175a).
- **`file_stem` becomes `pub(crate)`** in `relation.rs`, for `parquet_writer.rs`
  (contract 0de175a).
- **`finish` removes `.part` itself on a close or rename error.** It takes the
  `ArrowWriter` out before closing, so `Drop` no longer sees it. Every error still leaves no
  file.
- **`output_dir` with an explicit parent never fails** (the design session's decision; contract
  0de175a). The drafted version required a platform cache dir even under `--output-dir`,
  copying `run_gen_lubm`. A public resolver should be total (CLAUDE.md priority 5). That
  path has no test, because simulating a missing cache dir means mutating process-wide
  environment variables in a parallel test run.
- **`GeneratedMeta` needs no `#[serde(bound)]`.** serde_derive infers the flattened field's
  bounds. It derives `PartialEq` but not `Eq`, because `E` may be `serde_json::Value`.
- **The temporary `#[allow(dead_code)]`** on `RelationSink::new`, `into_records` and
  `COMMON_FIELDS` (Tasks 3.1 and 3.2) stays an `allow` and is removed by Task 3.3.
  `#[expect(dead_code)]` would go unfulfilled in `--all-targets` test builds, where the unit
  tests use these items, and fail clippy.
- **Extra tests beyond the contract** (drafter): `accessors_return_the_common_fields`, and
  five `run` unit tests and `no_queries_write_no_meta` (duplicate query names, a predicate over an unwritten relation, a
  malformed rule, non-object extras, and each common field refused as an extra).
- **Not checked:** an atom's arity against its relation's written columns. The spec asks
  `run` only for relation names; an arity mismatch still surfaces at `bench run`'s
  validation.
- **Normalised:** story and task ids are numeric (`Story 2`, `Task 2.1`); every test run
  uses `cargo test`, because the dev shell has no nextest; commit trailers are this
  plan's.

**Plan review.**

A code-quality-reviewer read the merged plan's code before commit (no cargo). Fixed here:
- **Important.** Nothing pinned that `meta.json` is written last: every failure test failed
  before step 5. `ToyGenerator` gained `no_queries`, and `no_queries_write_no_meta` fails in
  step 5's `validate()`, so an early marker fails the test. Task 3.3's second mutant checks
  exactly that.
- **Important.** Story 4's check counted lines with `grep -c`, so a correct CLAUDE.md edit
  would have read 4, not 6. It now counts occurrences with `grep -o … | wc -l`.
- **Decision: an empty batch is a no-op at any arity** (the design session's; contract
  f7e21c1). It follows the repo convention that an empty batch is accepted under any header.
  `append` keeps the `failed` check first, and `empty_batch_of_any_arity_is_a_no_op` pins the
  rule.
- **Minor:**
  - Task 3.3's first mutant replaces `clear_commit_marker` with `fs::create_dir_all`, so only
    the marker deletion goes; it is restored by copy-aside and `cmp`.
  - Mutant 4's failure is restated: it fails inside `invalid_message`.
  - Story 1's commit trailers were corrected.
  - The expected output is one `test result:` line per target.
  - The docs no longer claim a failed writer "leaves neither file": `<path>.part` is removed
    and `path` is not written, though a `path` from an earlier run can remain.
  - "The first three" guarantees are pinned, not four.
  - `WRITE_CHUNK_ROWS` is "the writer's own" buffering.
  - The CLAUDE.md row orders the checks as the code does.
  - Task 3.1 leaves two `pub(crate)` items, not one.
- **Checked and sound:**
  - batch independence, against parquet 59.3.0's page and dictionary checks, which count
    from each `write` call;
  - mutants 1–3 of Task 1.3;
  - `finish(mut self)` with `Drop`;
  - imports, intra-doc links and dead code per task;
  - the serde flatten round-trip;
  - every anchor line in the tree.

