# Flat Join Output: Answers as Relations

**Date:** 2026-10-23
**Status:** Design approved in conversation (2026-10-23), decision by decision.
**Scope:** Every API that collects a join's result returns it flat. `kermit::db::lftj_join` and
`hash_join` return the answer as a relation, the `(RelationHeader, Tuples)` pair that
`Catalog::new` takes: a header named after the query's head with one column per head variable,
and one flat `Tuples` batch of the head's arity. `compute_join` returns the `Tuples`.
`JoinAlgo::join_iter` is deleted. `prepare` decides the answer's header
(`Prepared::answer_header`), and `kermit join`'s CSV header row comes from it. The streamed path
is untouched: `JoinAlgo::join_for_each`, `kermit::db::execute` and every sink in the binary keep
their code, so no timed path, metric, report or measurement changes.
**Related:**
- #111 and its spec (`docs/specs/2026-10-08-flat-tuple-batch-design.md`), which made input flat
  and put "the join's output representation" out of scope;
- #65 and its spec (`docs/specs/2026-10-02-streamed-join-iteration-design.md`), which made the
  push sink the join's primitive and rejected a lending pull iterator;
- #110 (hybrid plans) and #108 (parallel probe), which would consume answers as relations or as
  per-worker batches; neither is part of this change;
- kraken (`aannleax/kraken` at 974b36b) and the Nemo it vendors, read for comparison (see
  Comparison).

## Summary

Since #111 every relation enters kermit as one flat `Tuples` batch with a `RelationHeader`, but
every collected result leaves as `Vec<Vec<usize>>`: one heap allocation, and one 24-byte `Vec`
header, per row. This change makes the output end match the input end. A collected answer is a
relation in exactly the form the catalog loads: its header names the head predicate and its
variables, and its rows are one flat buffer.

The lifecycle keeps its steps. `execute` stays the one step that runs the join, and collecting
is one sink among three, beside the counter the benchmarks time and the CSV writer. Nothing in
the binary collects and no timed function changes, so this is an API and memory change for
tests and library callers, not a performance change.

## Motivation

- **Input is flat, output is not.** The collecting functions (see Background) build
  `Vec<Vec<usize>>`. Each row costs a malloc and a free, a 24-byte header in the outer vector
  and a separate heap chunk: 56 bytes per row at arities 1 to 3 under glibc's 32-byte minimum
  chunk, against 8 bytes per value flat (7× at arity 1, 3.5× at arity 2, 2.3× at arity 3;
  worked out from glibc's chunk sizes, not measured).
- **The answer's shape is split.** Its arity is `Prepared::head_len()`, and its column names are
  re-derived by the binary from the parsed query (`head_column_names`,
  `kermit/src/main.rs:418`), whose arms for constants and `_` in the head are unreachable now
  that validation rejects both.
- **An answer cannot be handed to anything that takes relations.** The catalog, the readers and
  `ParquetWriter::write_batch(&Tuples)` all take `(RelationHeader, Tuples)` or `Tuples`; a
  collected answer is neither.
- **`join_iter` misleads.** It returns an `Iterator`, but it runs the whole join and collects
  every row before yielding the first (`kermit-algos/src/join_algo.rs:48`).

Not a motivation: speed. The benchmarks never collect (`iteration`, `end_to_end` and `--verify`
count through `ExecutionFamily::count_rows`, and `kermit join` streams CSV), and the joins
already allocate nothing per row (#65).

## Decisions

| Question | Decision |
|---|---|
| What becomes flat? | Every collected answer. The streamed path keeps its code. |
| Should the algorithms write rows straight into the output buffer? | No. It saves one copy per row, but needs either a second traversal beside `join_for_each` (#65 rejected a `join_count` for that reason) or a timed path whose memory grows with the result, which undoes #65. |
| Should fixed-size flat batches become the unit a sink receives? | No. It redefines what `iteration` measures (a schema bump and a full re-baseline), adds a buffering layer the papers don't have, and its payoffs (bulk Parquet output, per-worker output for #108) are for features that don't exist yet. |
| Should a join become something you pull rows from? | No. Push stays the primitive, as #65 decided. Kraken pulls (`TrieJoin::advance` / `current`), but every kraken join is a leapfrog join; HashTriejoin's recursive Algorithm 3 would need an explicit-state rewrite. |
| Does the lifecycle gain a step? | No. `execute` is the one terminal step, and collecting is `execute` with a `Tuples` sink. A dedicated `collect` step was considered and dropped: a caller holding a `Prepared` can size the sink itself, and one terminal step keeps what is timed unambiguous. |
| Is a collected answer rows or a relation? | A relation: its header and its rows. |
| A pair or a struct? | The plain pair `(RelationHeader, Tuples)`, the element type of `Catalog::new`'s input, so an answer goes back in with no conversion. |
| Who decides the answer's header? | `prepare`, through `Prepared::answer_header()`. The header is derived from the executed query's head on each call, not stored, so `Prepared`'s fields, and therefore `execute`'s code, stay as they are. |
| What is the header? | Named after the head predicate, with one attribute per head variable, in head order. |
| `JoinAlgo::join_iter`? | Deleted, not renamed. `join_for_each` is the trait's only method; collecting at that layer is a `Tuples` sink of arity `plan.variable_ordering.len()`. |
| `compute_join`? | Returns `Tuples` alone. Its inputs are bare batches under synthetic names (`R0`, `V0`, …): headerless in, headerless out. |
| An answer of more than `RowId::MAX` rows (about 4.29 × 10⁹)? | Panics, as `Tuples::push` does, documented with "stream it with the `_for_each` entry points instead". An error variant cannot stop the join early (the sink cannot say stop), so it would arrive only after the whole join had run. |
| Where does the CSV header row come from? | `Prepared::answer_header().attrs()`. `head_column_names` and its test are deleted. The CSV bytes are unchanged. |
| A `materialise` metric (kraken's `execute_materialize`)? | Not now. It would time `execute` with a `Tuples` sink, and is worth adding only for a comparison with kraken's materialising mode. |

## Background: what collects today

| Function | Returns | Callers |
|---|---|---|
| `JoinAlgo::join_iter` (`kermit-algos/src/join_algo.rs:48`), provided | `impl Iterator<Item = Vec<usize>>`: every variable, collected before the first is yielded | four HashTriejoin unit tests (`kermit-algos/src/hash/hash_triejoin.rs:479-529`) |
| `kermit::db::lftj_join` (`kermit/src/db.rs:91`) | `Result<Vec<Vec<usize>>, JoinError>`: head rows | `kermit/src/db.rs` tests |
| `kermit::db::hash_join` (`kermit/src/db.rs:126`) | as `lftj_join` | `kermit/src/db.rs` tests |
| `kermit::compute_join` (`kermit/src/lib.rs:53`) | `Vec<Vec<usize>>`: head rows | `kermit/src/lib.rs` tests |
| the test harness's `JoinEntry::join` (`kermit/tests/common/utils.rs:29`) | `Result<Vec<Vec<usize>>, JoinError>` | the join suites, through `join_under_planner` and `test_join` |

Nothing in the binary collects. `iteration`, `end_to_end` and `--verify` count through
`count_rows` (`kermit/src/execution.rs:477`), and `kermit join` / `bench join --output` stream
through `CsvSink` (`kermit/src/main.rs:435`).

## Design

### The lifecycle (`kermit::db`)

The steps are unchanged; collecting is a sink.

```
Catalog::new → prepare → build → execute(prepared, indexes, sink)
                                    ├── counter   count_rows: iteration, end_to_end, --verify
                                    ├── CsvSink   kermit join, bench join --output
                                    └── Tuples    lftj_join, hash_join, compute_join, the harness
```

A caller that runs the steps itself collects in four lines:

```rust
let prepared = prepare(&catalog, &query, &planner)?;
let indexes = build::<R>(&catalog, &prepared);
let mut rows = Tuples::new(prepared.head_len());
execute::<R, JA>(&prepared, &indexes, |row| rows.push(row));
```

The arity comes from `Prepared`, not from the first row, so an empty answer keeps the head's
arity. `execute`'s semantics carry over unchanged: each row is the head's columns, each answer
appears `multiplicity` times, and a query whose atoms are all constant checks yields the empty
row once per answer (arity 0, length `multiplicity`).

### `Prepared::answer_header` (`kermit/src/db/lifecycle.rs`)

```rust
/// The answer's header: named after the head predicate, with one attribute
/// per head variable, in head order. Built from the executed query's head on
/// each call; its arity is [`head_len`](Self::head_len).
pub fn answer_header(&self) -> RelationHeader
```

`prepare` has validated the head, so every head term is a distinct variable and the attribute
names are distinct. An empty head gives a header with no attributes and arity 0. `Prepared`
gains no field: the header is rebuilt from `self.query.head` (the head as written) whenever it
is asked for, which happens once per collected answer and once per CSV, both untimed.

### Collecting (`kermit::db`, `kermit`)

- A private `collect_join::<R, JA>(catalog, query, planner) -> Result<(RelationHeader, Tuples),
  JoinError>` sits beside `run_join` in `kermit/src/db.rs`: `prepare`, `build`, `execute` with a
  `Tuples` sink, then `Ok((prepared.answer_header(), rows))`.
- `lftj_join<R, JA>` and `hash_join<R>` return `Result<(RelationHeader, Tuples), JoinError>`
  through it. Their bounds and errors are unchanged.
- `compute_join<R, JA>` returns `Tuples`, the pair's second half.
- Each documents under `# Panics` that an answer of more than `RowId::MAX` rows panics, and that
  such a result must be streamed with `lftj_join_for_each` / `hash_join_for_each`.

### `JoinAlgo` (`kermit-algos/src/join_algo.rs`)

- `join_iter` is deleted. `join_for_each` is the trait's one method.
- `join_for_each`'s doc gains one sentence: to collect, pass a sink that pushes into
  `Tuples::new(plan.variable_ordering.len())`.
- The four HashTriejoin unit tests that used `join_iter` collect through a helper in their test
  module that does exactly that.

### The binary (`kermit/src`)

- `write_prepared_join` (`kermit/src/main.rs:499`) takes the CSV header row from
  `prepared.answer_header().attrs()`.
- `head_column_names` (`kermit/src/main.rs:418`) and its unit test
  (`head_column_names_extracts_variables_atoms_and_placeholders`) are deleted.
- Nothing else changes: `ExecutionFamily::{build_indexes, execute, count_rows}`, `CsvSink`, the
  runners and every metric keep their code. The binary gains no collecting method, since nothing
  in it collects.

### Unchanged

- `JoinAlgo::join_for_each` and both implementations.
- `kermit::db::{prepare, build, build_with, execute}`, `Prepared`'s fields (it gains only the
  `answer_header` method) and `Indexes`.
- `lftj_join_for_each` and `hash_join_for_each`.
- `TrieIteratorWrapper`'s per-row `Iterator`, which scans a trie and is not a join's output.
- The report schema (v5) and kermit-lab.

## Comparison: kraken and Nemo

- **Kraken** (`aannleax/kraken` at 974b36b) defines a join in code, as tables plus one layer per
  variable, with no head; a row is every variable in layer order, unnamed. Its timed mode is
  `execute_stream`, `while join.advance() { n += 1 }`. `execute_materialize` appends each row to
  a flat `TupleBuffer` (the type it loads CSVs into), returns `len()` and drops the buffer, so
  results never leave the system. The harness measures one mode per system: stream where
  supported. Kraken's trie systems pull rows (`TrieJoin::advance` / `current`) and are all
  leapfrog joins; kraken has no hash join.
- **Nemo**, vendored in kraken and the model for kraken's `coco` tries, makes a rule's answer a
  table. `Trie::from_partial_trie_scan` streams the join's rows into column builders, and
  `execute_plan` registers each permanent result under the rule head's name
  (`nemo/nemo-physical/src/management/database.rs:484`), where later rules read it.
- **Kermit after this change** has Nemo's shape in kermit's own storage format: the answer is a
  named relation, stored flat, as the catalog stores relations. Its semantics stay kermit's
  where they differ from Nemo's: kermit projects to the head and keeps duplicates (bag), where
  Nemo cuts trailing layers and deduplicates (set); and kermit's answers arrive unsorted in
  general, which is fine for a catalog relation, since every index build sorts or hashes its
  input. Kermit's `iteration` times a counted streamed join, the same thing kraken's stream mode
  times.
- **The paper** (Freitag et al., VLDB 2020) is untouched: HashTriejoin still emits each tuple to
  its sink, as Umbra pushes each tuple to the next operator.

## Placement under the standard

Not an optimisation under `docs/specs/optimization-standard.md`: no structure, algorithm,
Layout, Config or BuildMode changes and no timed path changes, so there is no axis and no new
suite invocation. Under Priority 6 this is a cross-cutting change with its own explicit scope
(one output representation across the API), and it changes no sibling's behaviour.

## CLI, reports and kermit-lab

- `kermit join` and `bench join --output` write the same bytes: the header row's names are the
  head's variables, as before.
- No report field, axis or metric changes, so `REPORT_SCHEMA_VERSION` stays 5 and kermit-lab is
  untouched.

## Testing

- **The harness.** `JoinEntry::join` returns `Result<(RelationHeader, Tuples), JoinError>`,
  collecting with a `Tuples` sink. `join_under_planner` checks the header against the query (its
  name is the head predicate's, its attributes are the head's variables, its arity is the
  batch's) and returns `(Tuples, Vec<IndexSpec>)`, so every suite pattern, in every structure ×
  algorithm × optimiser cell, checks the answer's header. The six integration tests that read
  only `rows.len()` (H2O, SNAP, JOB, both LUBM tests and WatDiv) are unchanged;
  `subject_position_constant.rs` compares `to_vecs()`. `test_join` keeps its `vec![vec![…]]`
  expectations and compares the sorted `to_vecs()`.
- **`all_orders.rs`** collects with a `Tuples` sink of arity `prepared.head_len()` and compares
  the sorted `to_vecs()` with the oracle.
- **`kermit::db` unit tests** compare the pair. New ones pin:
  - the header: name `Q` and the head's variables in head order, including a head that reorders
    the body's variables (`Q(Y, X) :- p(X, Y).`);
  - an empty answer keeps the head's arity, which `Vec<Vec<usize>>` could not express;
  - an answer whose atoms are all constant checks has arity 0 and one row per answer: one on a
    sorted trie, `multiplicity` on a `HashTrie`. The parser cannot write an empty head, so the
    query is built by clearing a parsed one, as
    `a_query_with_no_atom_left_emits_the_empty_head_per_unit_of_multiplicity`
    (`kermit/src/db/lifecycle.rs`) does;
  - an answer re-enters a `Catalog` as a relation: a second query reads it and gets the expected
    rows, with no conversion, under both families.
- **`Prepared::answer_header`** has unit tests beside `prepare`'s, for a reordered head and an
  empty head.
- **`compute_join`'s tests** compare `to_vecs()`.
- **The HashTriejoin unit tests** collect through their helper; their assertions are unchanged.
- **`result_allocation.rs`** gains one cell per valid (structure, algorithm) pair at the default
  Layout: TreeTrie and ColumnTrie under LFTJ, HashTrie under HashTriejoin. Each collects
  `QUERY`'s answer, prepared and built outside the measurement, at the file's two result sizes
  (100 and 100,000 rows), and requires the large count to exceed the small one by at most 20:
  the buffer's doublings, ⌈log₂ 1000⌉ = 10, with 2× slack. A per-row allocation would add about
  10⁵. `allocation-counter` counts a reallocation as an allocation (its allocator implements no
  `realloc`, so the default one allocates), so every doubling is counted. The plan checks that
  each new cell fails under a mutant that allocates per row (for example, `row.to_vec()` kept
  alongside each push), and that the mutant applied.
- **CLI.** `cli_join_tests.rs` already pins the CSV header row, and passes unchanged.

## Measurement

None. No timed source changes and `Prepared`'s fields are unchanged, so `execute` compiles
from the same code. As a check, the plan compares the disassembly of the release binary's
`count_rows` and `build_indexes` instantiations before and after, with addresses normalised,
and records whether they are identical. A difference there would be codegen placement, which
the ±3% codegen bound accepted on 2026-10-04 covers; it would not trigger a re-measurement.

## Out of scope

- Pulling rows from a join: a lending iterator, and HashTriejoin's explicit-state rewrite. This
  would revisit #65's decision.
- Fixed-size flat batches as the unit a sink receives.
- A `materialise` metric.
- Consumers of answers as relations: a catalog insertion API, Parquet output for `kermit join`,
  multi-rule programs, #110's intermediates and #108's per-worker output.
- Stopping a join early (`ControlFlow` sinks).
- Lifting `Tuples`' `RowId::MAX` cap for answers.
- Test-local `Vec<Vec<usize>>` fixtures, expectations, oracles and collecting closures
  (`kermit/tests/common/oracle.rs`, and the `kermit/src/db/lifecycle.rs` and
  `kermit/src/execution.rs` tests), which test `execute` and are not output APIs.

## Sequencing

One change, with no dependency. A suggested order, each step green:

1. `Prepared::answer_header` and its tests.
2. The collectors' return types, with the harness and every test that reads them.
3. `join_iter` deleted.
4. The CSV header taken from `answer_header`; `head_column_names` deleted.
5. The `result_allocation.rs` cells.
6. The documentation.

## Acceptance

- No public function in `kermit-algos`, `kermit::db` or `kermit`'s library returns
  `Vec<Vec<usize>>`.
- `lftj_join` and `hash_join` return `(RelationHeader, Tuples)`: the header named after the head
  predicate with the head's variables as attributes, and the batch at the head's arity, also
  when it is empty. `compute_join` returns `Tuples`.
- `JoinAlgo` has one method, `join_for_each`.
- An answer re-enters a `Catalog` unchanged (unit test).
- `kermit join`'s CSV output is byte-identical (`cli_join_tests.rs` passes unchanged).
- The new `result_allocation.rs` cells pass, and a per-row-allocation mutant fails them.
- No timed function and no `Prepared` field changes; `REPORT_SCHEMA_VERSION` stays 5.
- The gate passes: `cargo test`, `cargo clippy --all-targets` under `-Dwarnings`,
  `nix develop --command cargo fmt --all --check`, `cargo doc --workspace` under `-Dwarnings`,
  `cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`, and the python
  job.

## Documentation

- `CLAUDE.md`: the `kermit` entry of Workspace Architecture (the collectors return the answer as
  a `(RelationHeader, Tuples)` relation); "Adding a new join algorithm" step 2 (drop "`join_iter`
  is provided"; collecting is a `Tuples` sink); the "CLI join CSV header" gotcha
  (`Prepared::answer_header` replaces `head_column_names`); Testing Patterns
  (`join_under_planner` checks the answer's header).
- `ARCHITECTURE.md`: §JoinAlgo Trait (the prose and the code block lose `join_iter`); §Catalog
  and lifecycle (`Prepared::answer_header`, the collectors' return types, collecting as a sink).
- `docs/specs/benchmarking-architecture.md:101`: `head_column_names(query)` becomes
  `Prepared::answer_header`.
- `docs/algorithms/hash-triejoin.md` lines 45 and 48: the collision tests run end to end through
  `join_for_each`, and the `join_iter` sentence goes.
- `kermit-algos/README.md:31`: the `join_iter` sentence goes.
- `kermit/README.md` lines 13–16 and the crate docs of `kermit/src/lib.rs` and
  `kermit/src/db.rs`: the collectors return relations, and `compute_join` returns `Tuples`.
