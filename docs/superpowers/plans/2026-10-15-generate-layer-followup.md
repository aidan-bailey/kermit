# Shared Generator Layer, Review Follow-up (#95, push 1b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the seven minor findings of push 1's final review (spec Amendment 1) before #96 and #97 build on the layer.

**Architecture:** One story in four tasks:
1. `run`: a step 0 target check, and an arity check on query atoms.
2. `RelationSink` / `RelationWriter`: error contexts naming the relation and file, and `Debug`.
3. `ParquetWriter`: two untested failure paths get tests, a compile-time chunk-size assertion, and `Debug`.
4. Two doc lines that described code ahead of its arrival.

There are no name changes and no new dependencies.

**Tech Stack:** Rust nightly in the nix dev shell; `anyhow` contexts; `std::fmt::Debug` by hand.

**Spec:** `docs/specs/2026-10-15-h2o-benchmark-design.md`

The spec is at bbaef2e, Amendment 1. **Names contract:** `docs/superpowers/plans/2026-10-15-generate-layer-contract.md` (bbaef2e; `check_target` and the arity rule are marked "push 1b").

---

## Ground rules for the executor

The ground rules of `docs/superpowers/plans/2026-10-15-generate-layer.md` apply unchanged: paths, branch, host checks, builds, the floated toolchain override, formatting, doc comments, no `unsafe`, staging and committing by path, and trailers. In short:
- `WT=/tb/Source/Academia/kermit/.loom/worktrees/aidanb/h2o_18dea44a38dbca15`, branch `aidanb/h2o`. Never switch branches, amend or push.
- **Host:** cargo runs only in a slot the benchmark co-ordinator session allots. Check the host first. The #101 chain's scripts must be stopped (`ps` stat `T`) or gone, and `pgrep -af "[k]ermit-bench-runs/.*/bin/kermit"` must print nothing.
- Every cargo command is `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo …`. Never commit `flake.lock` or `Cargo.lock`.
- Commit by pathspec: `git commit -m "…" -- <files>`. The untracked guard copy `scripts/hooks/developer_git_guard.py` is never committed, edited or deleted.
- This plan may be applied in "edit now, verify later" mode only if the user approves it for this plan. Otherwise every task's run steps run in order, in a cargo slot.

## Story 1: Review follow-ups

- **Developer:** intermediate-dev
- **Files:** `kermit-generate/src/run.rs`, `kermit-generate/tests/run.rs`, `kermit-generate/src/sink.rs`, `kermit-generate/src/lib.rs`, `kermit-ds/src/parquet_writer.rs`, `ARCHITECTURE.md`
- **Interfaces:** no public name changes. `run` gains a private `check_target` (step 0) and an arity check in `check_queries`; `ParquetWriter` and `RelationWriter` gain `Debug`.
- **Verify:** `nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate -p kermit-ds`

### Task 1.1: `run` checks its target first, and each atom's arity

**Files:**
- Modify: `kermit-generate/src/run.rs`
- Modify: `kermit-generate/tests/run.rs`

- [ ] **Step 1: Write the failing tests.** In `kermit-generate/src/run.rs`'s `mod tests`, add after the last test:

  ```rust
      #[test]
      fn an_atom_needs_one_term_per_column() {
          let records = [record("edge")];
          let wide = [query("q3", "Q(X, Y) :- edge(X, Y).")];
          let err = check_queries(&wide, &records).unwrap_err().to_string();
          assert!(err.contains("q3"), "{err}");
          assert!(err.contains("edge"), "{err}");
          assert!(err.contains('2') && err.contains('1'), "{err}");
      }

      #[test]
      fn the_output_directory_must_end_in_the_bench_name() {
          check_target(Path::new("/cache/toy-bench"), "toy-bench").unwrap();
          let err = check_target(Path::new("/cache/other"), "toy-bench")
              .unwrap_err()
              .to_string();
          assert!(err.contains("toy-bench"), "{err}");
          assert!(check_target(Path::new("/cache/toy-bench"), "").is_err());
          assert!(check_target(Path::new("/cache/a/b"), "a/b").is_err());
      }
  ```

  In `kermit-generate/tests/run.rs`, add after the last test. A mismatched target fails before step 1, so it must leave the stale `meta.json` in place and write no relation:

  ```rust
  /// Step 0 runs before anything is touched: a target whose directory does
  /// not end in the bench name fails, and the planted `meta.json` survives.
  #[test]
  fn a_mismatched_target_touches_nothing() {
      let dir = tempfile::tempdir().unwrap();
      let toy = ToyGenerator::new(dir.path().join("elsewhere"));
      plant_stale_meta(&toy.out_dir);
      let message = format!("{:#}", run(&toy).expect_err("run should have failed"));
      assert!(message.contains("toy-bench"), "{message}");
      assert!(meta_json_path(&toy.out_dir).exists(), "step 0 must not touch meta.json");
      assert!(!toy.out_dir.join("edge.parquet").exists(), "no relation may be written");
  }
  ```

- [ ] **Step 2: Run them to see them fail.**
  ```bash
  nix develop --override-input rust-overlay github:oxalica/rust-overlay --no-write-lock-file --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-generate
  ```
  Expected: a compile error, `cannot find function check_target in this scope`, in the unit tests.

- [ ] **Step 3: Implement.** In `kermit-generate/src/run.rs`:

  Replace the import line
  ```rust
      std::{collections::HashSet, fs, io::ErrorKind, path::Path},
  ```
  with
  ```rust
      std::{collections::HashSet, ffi::OsStr, fs, io::ErrorKind, path::Path},
  ```

  In `run`'s doc, replace
  ```rust
  /// The steps and their order are the cache contract:
  ///
  /// 1. Create the output directory and delete any existing `meta.json`, so that
  ```
  with
  ```rust
  /// The steps and their order are the cache contract:
  ///
  /// 0. Check the target before anything is touched: the output directory must
  ///    end in the benchmark name, where discovery and `materialize` look.
  /// 1. Create the output directory and delete any existing `meta.json`, so that
  ```
  and replace
  ```rust
  /// 4. Check the queries: names are unique, each one parses as a Datalog rule,
  ///    and each body predicate names a relation the generator wrote. A malformed
  ///    rule therefore fails here, not at `bench run`.
  ```
  with
  ```rust
  /// 4. Check the queries: names are unique, each one parses as a Datalog rule,
  ///    and each body predicate names a relation the generator wrote, with one
  ///    term per column. A malformed rule therefore fails here, not at
  ///    `bench run`, and no cache is committed for it.
  ```

  In `run`'s body, replace
  ```rust
      let out_dir = target.out_dir;

      clear_commit_marker(out_dir)?;
  ```
  with
  ```rust
      let out_dir = target.out_dir;

      check_target(out_dir, target.bench_name)?;
      clear_commit_marker(out_dir)?;
  ```
  and replace
  ```rust
      write_meta_json(out_dir, &meta)?;
      Ok(meta)
  ```
  with
  ```rust
      write_meta_json(out_dir, &meta)
          .with_context(|| format!("writing {}", meta_json_path(out_dir).display()))?;
      Ok(meta)
  ```

  Add, directly above `/// Step 1: creates `out_dir` and deletes `meta.json` if there is one.`:
  ```rust
  /// Step 0: `out_dir` must end in a component equal to `bench_name`, which
  /// is where discovery, `materialize` and
  /// `kermit_bench::cache::relation_cache_path` look for the cache. The full
  /// name rules are checked by `validate()` at step 5.
  fn check_target(out_dir: &Path, bench_name: &str) -> anyhow::Result<()> {
      if bench_name.is_empty() || out_dir.file_name() != Some(OsStr::new(bench_name)) {
          bail!(
              "the output directory {} does not end in the benchmark name {bench_name:?}",
              out_dir.display()
          );
      }
      Ok(())
  }

  ```

  In `check_queries`, replace
  ```rust
          for predicate in &rule.body {
              if !records.iter().any(|record| record.name == predicate.name) {
                  bail!(
                      "the query {:?} names the relation {:?}, which the generator did not write",
                      query.name,
                      predicate.name
                  );
              }
          }
  ```
  with
  ```rust
          for predicate in &rule.body {
              let Some(record) = records.iter().find(|record| record.name == predicate.name)
              else {
                  bail!(
                      "the query {:?} names the relation {:?}, which the generator did not write",
                      query.name,
                      predicate.name
                  );
              };
              if predicate.terms.len() != record.columns.len() {
                  bail!(
                      "the query {:?} gives the relation {:?} {} term(s), but it was written \
                       with {} column(s)",
                      query.name,
                      predicate.name,
                      predicate.terms.len(),
                      record.columns.len()
                  );
              }
          }
  ```
  Also update the `check_queries` doc line `/// Step 4: unique names, well-formed rules, and body predicates that name` / `/// written relations.` to:
  ```rust
  /// Step 4: unique names, well-formed rules, and body predicates that name
  /// written relations with one term per column.
  ```

- [ ] **Step 4: Run them to see them pass.** Run the same command as Step 2. Expected: every `kermit-generate` test passes (17 unit, 9 integration), including the three new ones.

- [ ] **Step 5: Commit.**
  ```bash
  git commit -m "fix(generate): check run's target first and each atom's arity (#95)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/run.rs kermit-generate/tests/run.rs
  ```

### Task 1.2: The sink's errors name the relation and file, and `RelationWriter` is `Debug`

**Files:**
- Modify: `kermit-generate/src/sink.rs`

- [ ] **Step 1: Write the failing tests.** In `kermit-generate/src/sink.rs`'s `mod tests`, add after the last test. It reuses the module's existing imports; add any of `RelationHeader` / `Tuples` the module lacks:

  ```rust
      #[test]
      fn errors_name_the_relation_and_its_file() {
          let dir = tempfile::tempdir().unwrap();
          let mut sink = RelationSink::new(dir.path());
          let positional = RelationHeader::new_positional("edge", 2);
          let message = format!("{:#}", sink.open(&positional).unwrap_err());
          assert!(message.contains("edge.parquet"), "{message}");

          let node = RelationHeader::new("node", vec!["id".to_string()]);
          let mut writer = sink.open(&node).unwrap();
          writer.write_batch(&Tuples::from(vec![vec![1]])).unwrap();
          // A directory where the file must go makes the final rename fail.
          std::fs::create_dir(dir.path().join("node.parquet")).unwrap();
          let message = format!("{:#}", writer.finish().unwrap_err());
          assert!(message.contains("node.parquet"), "{message}");
          assert!(sink.records().is_empty());
      }

      #[test]
      fn a_relation_writer_debugs_its_relation() {
          let dir = tempfile::tempdir().unwrap();
          let mut sink = RelationSink::new(dir.path());
          let node = RelationHeader::new("node", vec!["id".to_string()]);
          let writer = sink.open(&node).unwrap();
          let shown = format!("{writer:?}");
          assert!(shown.contains("node"), "{shown}");
      }
  ```

- [ ] **Step 2: Run them to see them fail.** `… cargo test -p kermit-generate sink` (full prefix as in the Verify line). Expected: a compile error for `{writer:?}` (`RelationWriter` doesn't implement `Debug`). Once that is in place, the first test fails because the message lacks the file name.

- [ ] **Step 3: Implement.** In `RelationSink::open`, replace
  ```rust
          let writer = ParquetWriter::create(&path, header)?;
  ```
  with
  ```rust
          let writer = ParquetWriter::create(&path, header)
              .with_context(|| format!("creating the relation {name:?} at {}", path.display()))?;
  ```
  In `RelationWriter::write_batch`, replace
  ```rust
          Ok(self.writer.write_batch(batch)?)
  ```
  with
  ```rust
          self.writer.write_batch(batch).with_context(|| {
              format!("writing the relation {:?} to {}", self.header.name(), self.path.display())
          })
  ```
  In `RelationWriter::finish`, replace
  ```rust
          let rows = self.writer.finish()?;
          let sha256 = sha256_hex(&self.path)?;
  ```
  with
  ```rust
          let rows = self.writer.finish().with_context(|| {
              format!("finishing the relation {:?} at {}", self.header.name(), self.path.display())
          })?;
          let sha256 =
              sha256_hex(&self.path).with_context(|| format!("hashing {}", self.path.display()))?;
  ```
  (The closures capture `self.header` and `self.path` disjointly from the moved `self.writer`, under edition 2021's closure captures. `RelationWriter` has no `Drop`, so moving a field out is legal.)

  Add after the `impl RelationWriter<'_> { … }` block:
  ```rust
  /// Shows the relation and its `ParquetWriter`, but not the sink it borrows.
  impl std::fmt::Debug for RelationWriter<'_> {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          f.debug_struct("RelationWriter")
              .field("relation", &self.header.name())
              .field("writer", &self.writer)
              .finish_non_exhaustive()
      }
  }
  ```
  This needs Task 1.3's `Debug` for `ParquetWriter`. If Task 1.3 has not landed yet, do Task 1.3 first. The two tasks touch different files.

- [ ] **Step 4: Run them to see them pass.** Run the same command as Step 2. Expected: every `sink` test passes.

- [ ] **Step 5: Commit.**
  ```bash
  git commit -m "fix(generate): sink errors name the relation and its file; RelationWriter is Debug (#95)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/sink.rs
  ```

### Task 1.3: `ParquetWriter`'s untested failure paths, chunk-size assertion and `Debug`

Run this task before Task 1.2's Step 3, because `RelationWriter`'s `Debug` prints a `ParquetWriter`.

**Files:**
- Modify: `kermit-ds/src/parquet_writer.rs`

- [ ] **Step 1: Write the failing tests.** In `mod tests`, add after the last test:

  ```rust
      #[test]
      fn rejects_rows_beyond_row_id_max() {
          let dir = tempfile::tempdir().unwrap();
          let path = dir.path().join("edge.parquet");
          let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
          writer.rows = u64::from(RowId::MAX);
          let message = invalid_message(writer.write_batch(&rows_of(2, 1)));
          assert!(message.contains("exceed"), "{message}");
      }

      #[test]
      fn a_failed_finish_leaves_no_part_file() {
          let dir = tempfile::tempdir().unwrap();
          let path = dir.path().join("edge.parquet");
          let part = dir.path().join("edge.parquet.part");
          let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
          writer.write_batch(&rows_of(2, 3)).unwrap();
          // A directory at `path` makes the rename onto it fail.
          std::fs::create_dir(&path).unwrap();
          assert!(matches!(writer.finish(), Err(RelationError::Io(_))));
          assert!(!part.exists(), "a failed finish must remove the .part file");
          assert!(path.is_dir(), "the directory in the way is left alone");
      }

      #[test]
      fn a_writer_debugs_its_path_and_rows() {
          let dir = tempfile::tempdir().unwrap();
          let path = dir.path().join("edge.parquet");
          let mut writer = ParquetWriter::create(&path, &header_of("edge", 2)).unwrap();
          writer.write_batch(&rows_of(2, 3)).unwrap();
          let shown = format!("{writer:?}");
          assert!(shown.contains("edge.parquet") && shown.contains("rows: 3"), "{shown}");
      }
  ```

- [ ] **Step 2: Run them to see them fail.** `… cargo test -p kermit-ds parquet_writer`. Expected: a compile error for `{writer:?}` (`ParquetWriter` doesn't implement `Debug`). The first two tests already pass once the third compiles, because they pin behaviour that already exists, which is the point of finding 4.

- [ ] **Step 3: Implement.**
  - Replace
    ```rust
    /// Rows per row group; a multiple of `WRITE_CHUNK_ROWS`.
    pub(crate) const ROW_GROUP_ROWS: usize = 1 << 20;
    ```
    with
    ```rust
    /// Rows per row group; a multiple of `WRITE_CHUNK_ROWS`, as the assertion
    /// below enforces.
    pub(crate) const ROW_GROUP_ROWS: usize = 1 << 20;

    // A chunk must never straddle a row group, or `ArrowWriter`'s input would
    // depend on the batching after all.
    const _: () = assert!(ROW_GROUP_ROWS % WRITE_CHUNK_ROWS == 0);
    ```
  - In the struct doc, replace `/// Four guarantees, the first three each pinned by a test:` with `/// Four guarantees, the first three each pinned by tests:`.
  - Add, directly after the `pub struct ParquetWriter { … }` block:
    ```rust
    /// Shows the path, the rows written and whether the writer failed; the
    /// Arrow writer and the chunk buffers are left out.
    impl std::fmt::Debug for ParquetWriter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ParquetWriter")
                .field("path", &self.path)
                .field("rows", &self.rows)
                .field("failed", &self.failed)
                .finish_non_exhaustive()
        }
    }
    ```
  - In `mod tests`, the helper's comment `/// (`ParquetWriter` is not `Debug`, so `unwrap_err` is unavailable.)` is no longer true. Replace it with `/// (It takes any result, so tests need not name the success type.)`.

- [ ] **Step 4: Run them to see them pass.** Run the same command as Step 2. Expected: 14 `parquet_writer` tests pass.

- [ ] **Step 5: Commit.**
  ```bash
  git commit -m "test(kermit-ds): ParquetWriter's row limit and failed finish; Debug; chunk-size assert (#95)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-ds/src/parquet_writer.rs
  ```

### Task 1.4: Docs that described code ahead of its arrival

**Files:**
- Modify: `kermit-generate/src/lib.rs`
- Modify: `ARCHITECTURE.md`

- [ ] **Step 1: The crate docs.** In `kermit-generate/src/lib.rs`, replace
  ```rust
  //! 1. create the output directory and delete any old `meta.json`;
  ```
  with
  ```rust
  //! 1. check that the output directory ends in the benchmark name, then
  //!    create it and delete any old `meta.json`;
  ```
  and replace
  ```rust
  //! The binary's `materialize` dispatch calls `run` once per generator kind
  //! and discards the `GeneratedMeta` it returns. `bench gen` may print a
  //! summary from it.
  ```
  with
  ```rust
  //! The binary wires each non-RDF generator kind in as a `materialize`
  //! dispatch arm that calls `run`; the arms arrive with the generators
  //! (#95, #96, #97). An arm discards the `GeneratedMeta` that `run` returns,
  //! and `bench gen` may print a summary from it.
  ```

- [ ] **Step 2: ARCHITECTURE.md's tree line.** Replace
  ```text
  ├── kermit-generate/ # Non-RDF generators (H2O, SNAP, JOB) and their shared layer
  ```
  with
  ```text
  ├── kermit-generate/ # Layer shared by the non-RDF generators (H2O #95, SNAP #96, JOB #97)
  ```

- [ ] **Step 3: Check.** `… cargo doc -p kermit-generate --no-deps` with `RUSTDOCFLAGS=-Dwarnings` inside the `env`, which should be clean. Then `grep -cF 'Layer shared by the non-RDF generators' ARCHITECTURE.md`, which should print `1`.

- [ ] **Step 4: Commit.**
  ```bash
  git commit -m "docs(generate): describe the dispatch arms as arriving with the generators (#95)

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_012eFF4NmQ7DP2R9CU7WPXDD" -- kermit-generate/src/lib.rs ARCHITECTURE.md
  ```

## Final gate

In the co-ordinator's slot, with the host checked first, on the floated override and at 2 jobs:
1. `cargo fmt --all --check` is silent. Commit any formatting as its own `style:` commit by pathspec.
2. `RUSTFLAGS=-Dwarnings cargo clippy --all-targets` exits 0. The `unused dependency kermit-generate` warning for `kermit/Cargo.toml` is expected.
3. `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps` exits 0.
4. `cargo test -p kermit-generate -p kermit-ds` passes. These are the only crates whose code changed; the binary does not use `kermit-generate` yet.
5. Scope: `git diff --name-only bbaef2e..HEAD` lists only the Files above (plus this plan, if it was committed after bbaef2e).

## After this plan

The design session lands push 1b on the user's go-ahead: fetch, merge `origin/master` (never rebase), re-run the gate if the merge brought code, then `git push origin HEAD:master`. #96 and #97 hear before it lands that `run` is stricter: atom arity, and an `out_dir` ending in `bench_name`.
