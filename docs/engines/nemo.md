# `Nemo`

> **Status:** experimental · **CLI:** `--engine nemo` · **Feature:** `engine-nemo` · **Implementation:** [`kermit::engine::nemo`](../../kermit/src/engine/nemo.rs)

Nemo (`knowsys/nemo`, Apache-2.0) is a rule engine whose join is a leapfrog triejoin over
column-oriented tries, the same family as kermit's `ColumnTrie` under `LeapfrogTriejoin`. It is the
peer engine of thesis RQ1.4: a bounded sanity baseline with explicit threats to validity, not a
head-to-head claim. kermit runs it in process through the public API of `nemo-physical` at a pinned
git revision, with no fork, on the same `Prepared` query the structure cells run.

## Phase mapping

| Phase | What Nemo does | Timed | Notes |
|---|---|---|---|
| `load` | Shared `Arc` handles on the catalog's tuples of each relation the query reads | no | |
| `build` | A fresh `DatabaseInstance`; per `IndexSpec`, `register_table` and `add_source_external` with a `ViewProvider` that streams the spec's matching rows, output columns only, as integer values; then `load_trie_into_memory` per spec, which sorts, deduplicates and builds the trie. The join plan is built here, in kermit's variable order | yes | One copy of the rows, plus Nemo's sort, deduplication and trie: the same kind of work as `ColumnTrie`'s build |
| `iteration` | Not offered | | Upstream's streaming join is private; the public `execute_plan` stores its result as a deduplicated trie. The note `{label} does not stream, so it skips iteration and end-to-end` is printed |
| `materialize` | `execute_plan`, then `count_rows_in_memory` of the stored result | yes, opt-in | Nemo stores the result, so every sample runs on a fresh build in untimed setup (`MATERIALIZE_MUTATES`), as a lazy `HashTrie` does; a second `materialize` on one build panics |
| `space` | The sum of `table_size_bytes` over the spec tables | exact | Read in the describe step, before anything runs |

`Engine` constants: `NAME` is `"nemo"`, `STREAMS` false, `KEEPS_DUPLICATES` false,
`MATERIALIZE_MUTATES` true, `HAS_SPACE` true.

## Translation

Nemo's translator (`kermit/src/engine/nemo.rs`) builds one public `ExecutionPlan`:
- one `OperationTable` of result markers, pushed in **kermit's plan order** (`Prepared::plan()`), so
  Nemo runs kermit's variable order and not one of its own;
- one `fetch_table(markers, id)` per atom, over its spec's Nemo table (merged specs are one Nemo
  table, fetched by several atoms);
- `join_empty(result_markers)`, then `add_subnode` per atom, then `write_permanent`.

The result's columns follow the plan order, so `Engine::rows` (used by tests only) permutes them into
canonical order.

## Fairness caveats

- **Compare Nemo with kermit on `build`, `materialize` and `space`.** Nemo has no streamed
  `iteration`, so it is never compared on `iteration` or `end_to_end`; `kl.engines()` draws exactly
  those three pairs.
- **Set semantics.** Nemo's loader deduplicates the input and its result is a deduplicated trie, so
  its count equals the bag count only on duplicate-free inputs. kermit's `materialize` is defined to
  match: it collects the full assignments and counts distinct rows.
- **`materialize` is not a streamed join.** Nemo stores its result trie; kermit's cells and DuckDB
  copy rows into a `Tuples`, sort and count. The costs are alike in kind (a stored, ordered result),
  not equal.
- **Join order.** Nemo runs kermit's plan, so a difference is not a join-order difference. Engine rows
  carry the `plan` axis, which is that order: Nemo follows it (DuckDB does not).
- **A zero all-constant count skips the engine's build.** On a query with an all-constant atom whose
  count is 0 the result is known to be empty, so the engine cell skips its `build` entirely, while
  kermit's `build` still builds every index. The engine's `build` there is near zero and is not
  like-for-like.
- **Probes are single cold runs.** Compare them with other single runs, never with a Criterion
  estimate.
- **Memory.** Nemo does not stream, so its `--materialize-max-bytes` guard reads `expected` alone
  and runs before the describe step. Over the limit, nothing materialises: no preflight count, no
  describe-step count, no `materialize`, only the stderr note and the `materialize` metadata line.
  A `--verify`-only run (no `materialize` metric) over the limit materialises nothing either, and
  skips verification for that cell with the same note.
  A query without `expected` prints this warning once and runs unguarded:

  `warning: {label} does not stream and {benchmark}/{query} has no expected count, so materialize runs without the --materialize-max-bytes guard`

## Version pin

- `nemo-physical` is a git dependency in `kermit/Cargo.toml` at
  `rev = "04f0962b91edc1bbe73e6f65f08d41d1ee491980"` (workspace version 0.10.2-dev, `main` on
  2026-10-18), with `futures` (`executor::block_on`) and `async-trait` (`TableProvider`). None is in
  `default`; the feature is `engine-nemo`.
- `kermit::engine::nemo` exports `NEMO_REV` and `NEMO_PHYSICAL_VERSION`, and `Engine::version`
  returns `nemo-physical {NEMO_PHYSICAL_VERSION} @ {first 12 of NEMO_REV}`, which every Nemo report
  records as `engine_version`. A unit test pins `NEMO_REV` against the `rev` in `kermit/Cargo.toml`.
- Upstream is nightly-only, and at this revision it needs a rustc of 2026-11-02 or later:
  on the 2026-10-31 nightly it fails with `E0391` (rust-lang/rust#165263, fixed 2026-11-01). The
  flake's `rust-overlay` is locked past that.

To move the pin:
1. Change `rev` in the `nemo-physical` dependency of `kermit/Cargo.toml`, then `cargo update -p
   nemo-physical` to rewrite `Cargo.lock`.
2. Change `NEMO_REV` and `NEMO_PHYSICAL_VERSION` in `kermit/src/engine/nemo.rs` to the new revision
   and its workspace version. The unit test fails until they agree with `Cargo.toml`.
3. Re-check the public API the engine uses (`DatabaseInstance::register_table`,
   `add_source_external`, `load_trie_into_memory`, `ExecutionPlan`, `execute_plan`,
   `count_rows_in_memory`, `table_size_bytes`, `TableProvider`, `TupleWriter`): any of them going
   private or changing signature is a code change, not a pin move.
4. Run `cargo test -p kermit --features engine-duckdb,engine-nemo --lib`, the engine oracle and the
   acceptance probes below, and record the new revision here.

## Settings and limits

- `Settings` is `()`: Nemo has no kermit-side setting, and `--engine-threads` does not apply to it
  (its report carries no `engine_threads`; the flag is a usage error when no DuckDB engine is
  selected).
- An engine cell's preflight error names the benchmark, the query and the engine. `bench join
  --engine` runs the 64-byte Criterion group-name clash check over its kermit and engine cells, as
  `bench run` does.
- `--materialize-max-bytes` (default 4 GiB, range 1 MiB to 1 TiB) guards `materialize`. For Nemo,
  which does not stream, the row count is `expected` alone and the guard runs before the describe
  step, and a `--verify`-only run over the limit skips verification for the cell with the note (see
  Fairness caveats, Memory).
- kermit sets no timeout of its own, and an engine that fails ends its cell with an error; nothing is
  retried. No engine calls an outside service at run time: Nemo is a build-time git dependency.

## Tests

- `kermit/tests/engine_oracle.rs` checks Nemo's counts and contents against the **distinct** rows of
  the nested-loop oracle (distinct full assignments are the same set under set and bag semantics).
- Unit tests in `kermit/src/engine/nemo.rs` pin `NEMO_REV` against `kermit/Cargo.toml` and check that
  the join's markers follow kermit's plan order.
- The `engines` job of `.github/workflows/pr.yml` runs them with `engine-nemo`.

## See also

- [`duckdb.md`](duckdb.md) and [`TEMPLATE.md`](TEMPLATE.md).
- [`docs/data-structures/column-trie.md`](../data-structures/column-trie.md), Nemo's own family inside kermit.
- The design: [`docs/specs/2026-11-02-external-engines-design.md`](../specs/2026-11-02-external-engines-design.md).
