# Streamed Join Iteration

**Date:** 2026-10-02
**Status:** Approved design, not yet implemented
**Scope:** Issue #65. The `iteration` metric, the `end_to_end` metric's K
joins, and `bench run --verify` stop materialising the join result.

## Motivation

The `iteration` metric of `bench run` times `family.join(&engine, q)`. That
call ends in `JA::join_iter(...).collect()` (`kermit/src/db.rs:178`), so the
timed region builds the whole result as a `Vec<Vec<usize>>`. This has two
consequences:

1. **A result that doesn't fit in memory can't be benchmarked.** `q0005` of
   `watdiv-stress-100-test-1` returns 4,169,173,508 rows (DuckDB
   `COUNT(*)`). Materialised, that is roughly 300 GB. It is excluded by hand
   in `scripts/watdiv_stress_sample.py`.
2. **Per-row allocation pulls structure ratios toward 1.** Every cell pays
   the same per-row allocation, and that shared cost dominates exactly on
   the large-result queries. `BENCHMARKING.md` says the metric times "the
   join itself", which is not true today.

The two algorithms also allocate differently, which confounds algorithm
comparisons:

- LFTJ allocates twice per row: `TrieIteratorWrapper::next` clones its stack
  (`kermit-iters/src/trie.rs:161`), and `join_iter` collects a reordered
  copy.
- HashTriejoin builds its whole output before yielding anything. On the way
  it allocates per leaf visit (`chains`, `cursor`) and per candidate
  (`candidate`, plus two `Vec`s in `verify_and_construct`). Leaf chains
  usually hold one tuple, so leaf visits are roughly one per row.

`BatchSize::SmallInput` makes the memory problem worse: it keeps every output
of a batch alive until the batch has been timed.

## Decisions

| Question | Decision |
|---|---|
| Redefine `iteration` or add a metric? | Redefine `iteration`. It now consumes rows through a counting `black_box` sink. `end_to_end` changes the same way. |
| Result-size policy for `q0005`? | None. Once streamed, it runs like any other query. Verification measures one execution per cell (see Verification). |
| How the result is consumed | Push sink (internal iteration). `join_for_each(…, emit: impl FnMut(&[usize]))` is the primitive. |
| Return `Result` now, ahead of #78? | No. #78 owns the error design. Its later change is mechanical: `?` at about six non-test call sites. |

Two alternatives were rejected:

- **A per-algorithm `join_count`.** Each algorithm would carry a second
  traversal to keep in sync with its row path, and `black_box` would leak
  into `kermit-algos`.
- **A lending pull iterator.** HashTriejoin's recursive Algorithm 3 would
  have to become an explicit-stack state machine, at the cost of the
  readability that CLAUDE.md asks for (Priorities item 2).

## Design

### `kermit-iters`: `TrieIteratorWrapper`

- Add `pub fn advance(&mut self) -> Option<&[usize]>`. It holds today's
  three-phase traversal unchanged (backtrack, descend, arity filter) and
  returns `Some(&self.stack)` in place of `Some(self.stack.clone())`.
- `Iterator::next` becomes `self.advance().map(<[usize]>::to_vec)`, and the
  inherent `next` goes. This is behaviour-preserving, so TreeTrie and
  ColumnTrie iteration don't change.

### `kermit-algos`: `JoinAlgo<DS>`

```rust
pub trait JoinAlgo<DS: JoinIterable> {
    /// Calls `emit` once per result tuple, in canonical (head-first)
    /// variable order. The slice is borrowed for the duration of the call
    /// only, so the result is never materialised.
    fn join_for_each<F: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, emit: F,
    );

    /// Materialises the result through `join_for_each`.
    fn join_iter(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> { /* collect, then into_iter */ }
}
```

- The sink is generic, so it is monomorphised: there is no `dyn` call per row
  inside the timed region. `JoinAlgo` is never used as a trait object.
- The note "Laziness is not part of the contract" is replaced. The new text
  says that `join_for_each` is the streaming primitive for both algorithms,
  and that `join_iter` materialises.
- `QueryPlan` validation, and its panic, move into `join_for_each`.

**LFTJ.** `join_for_each` drives `LeapfrogTriejoinIter::new(…).into_iter()`
with `advance()`. It keeps one scratch `tuple` of length `arity`. For each
descent tuple it writes `tuple[v] = descent[descent_pos_of_var[v]]` and calls
`emit(&tuple)`.

**HashTriejoin.**

- `enumerate` threads `emit` where it threaded `output`.
- `emit_leaf` keeps the paper's cursor-based cross product (Algorithm 3,
  lines 16–19), with three changes:
  - The leaf chains are re-borrowed through `leaf_tuples()` instead of being
    collected into a `Vec`. Every implementation returns a borrowed slice.
  - `cursor`, the chain lengths and the candidate row are scratch buffers,
    allocated once per join and passed down the recursion.
  - `verify_and_construct` writes into the scratch row and returns `bool`
    instead of `Option<Vec<usize>>`.
- `join_for_each` replaces the eager `join_iter`.

### `kermit::db`

- `run_join` takes the sink and calls `JA::join_for_each`.
- The public API gains `lftj_join_for_each` and `hash_join_for_each`, which
  take the sink.
- `lftj_join` and `hash_join` keep their signatures and collect through the
  `_for_each` versions. Their existing callers and tests are unchanged, and
  they now exercise the streaming path.
- `test_join_panics_on_missing_relation` stays as it is. #78 owns turning
  that panic into an error.

### `kermit/src/execution.rs`: `ExecutionFamily`

| Method | Kind | Body |
|---|---|---|
| `join_for_each<S: FnMut(&[usize])>(&self, engine, query, emit: S)` | required | One line per family, into `lftj_join_for_each` / `hash_join_for_each`. |
| `count(&self, engine, query) -> u64` | provided | `black_box`es each row and increments a counter. |
| `join(&self, engine, query) -> Vec<Vec<usize>>` | provided | Collects. Kept for the `kermit join` and `bench join --output` CSV paths, which do need the rows. |

### `kermit/src/bench/run.rs`

- `--verify` compares `family.count(&engine, q)` with `expected`.
- `iteration` times `|q| family.count(&engine, q)`. A `SmallInput` batch now
  holds only `u64` outputs.
- `end_to_end` uses `black_box(family.count(&fresh, q))` for each of its K
  joins.

Row counts don't change, because nothing deduplicates or projects. Head
projection stays with #71.

### Keeping old and new reports apart

**Schema bump.**

- `REPORT_SCHEMA_VERSION` (`kermit/src/bench_report.rs`) and Python's
  `SCHEMA_VERSION` (`python/kermit-lab/kermit_lab/__init__.py`) go from 2 to
  3. The existing `include_str!` test pins the two together.
- A new v3 row in the change log of `docs/specs/bench-report-schema.md` says
  that `iteration` and `end_to_end` now time a streamed, counted join, and
  that their values are not comparable with v2.
- Only #65 bumps the version. #66 must not reuse "3".

**Mixing guard.**

A bump alone doesn't stop mixing. The loader rejects only versions *newer*
than it supports, so v2 and v3 reports would load into one DataFrame without
complaint. Therefore:

- `load_reports` raises `SchemaError` when one load contains reports from
  both sides of the boundary (some `schema_version < 3` and some `>= 3`).
  The message names one file from each side.
- `load_reports`, `kl.load` and `kl.load_samples` take a keyword
  `allow_mixed_schema=False` as a deliberate escape hatch. One use is putting
  `space` numbers side by side, since space and insertion semantics did not
  change.
- The CLI surfaces the error and gets no override flag.
- Loads that are all v2 keep working, so the analysis of the prelim sweep
  still loads.

### Documentation

| File | Change |
|---|---|
| `BENCHMARKING.md` | The Iteration and End-to-end rows say the join's rows pass through a counting `black_box` sink and no result rows are allocated. |
| `JoinAlgo` rustdoc, `docs/algorithms/leapfrog-triejoin.md`, `docs/algorithms/hash-triejoin.md`, `docs/algorithms/TEMPLATE.md` | `join_for_each` is the method each algorithm implements. The hash-triejoin "materialised, not streamed" bullet is rewritten. |
| `ARCHITECTURE.md` | Rewrite the paragraph on the `join_iter` laziness asymmetry. |
| `CLAUDE.md` | Name `join_for_each` where it names the algorithm trait method: Priorities item 3 and the add-an-algorithm recipe. |
| `docs/specs/bench-report-schema.md` | Current version is 3; add the change-log row. |
| `scripts/watdiv_stress_sample.py` | Remove the docstring's `--exclude q0005` advice. Keep the flag. |

## Out of scope

These are observations, not part of this change:

- **LFTJ allocates on every descent.** `update_iters` builds a `Vec<IT>` on
  each `open`/`up`, and `LeapfrogJoinIter::new` collects a permutation. This
  is algorithm cost, not result materialisation. Changing it would change
  LFTJ's measured cost, so it would be a separate change under the
  optimisation standard.
- **The hash selection view copies a filtered leaf chain on every leaf
  visit** (`kermit-algos/src/hash/selection.rs`). This is view work, and
  only queries with repeated variables pay it.
- **Streaming the CSV output of `kermit join` / `bench join --output`.** A
  sink that writes would need to propagate I/O errors. Nothing in #65 needs
  it.
- **Turning the join's panics into errors.** That is #78.

## Testing

**Breadth.** The standard suite's `test_join`
(`kermit/tests/common/utils.rs`) already runs through `lftj_join` /
`hash_join`, so every invocation is on the streaming path.

- Add `JoinEntry::count`, which drives the `_for_each` entry point with a
  counter.
- Assert that the count equals the expected row count.

This pins the counting sink for every structure × algorithm × optimiser ×
config invocation.

**Result-allocation test.** The new file `kermit/tests/result_allocation.rs`
installs a counting `#[global_allocator]`. Its counter is thread-local, so
parallel tests in the same binary don't disturb it.

- Query: `Q(X, Y, Z) :- R(X, Y), S(X, Z).` with |R| = 10 fixed, and |S|
  scaled so the result grows from 100 to 100,000 rows. The shared `X`
  exercises HashTriejoin's leaf verification.
- Cells: TreeTrie and ColumnTrie under LFTJ, and all four HashTrie Layout
  aliases under HashTriejoin.
- Assertion: the number of allocations during the counted join is the same
  at both sizes.
- Only the leaf fan-out varies, so the number of descents, and with it
  LFTJ's per-descent allocation, is constant. The doc comment says exactly
  what the test isolates.
- Mutation check: restore a per-row `to_vec`, confirm the mutation applied,
  and confirm that the test fails.

**Unit tests.**

- One test pins that `advance` and `next` yield the same tuples. The existing
  wrapper tests cover the traversal, since `next` now delegates to `advance`.
- The `verify_and_construct` tests move to the scratch-row signature, with
  the same cases.

**Python.** pytest cases for the mixing guard:

- a mixed v2/v3 load raises;
- `allow_mixed_schema=True` lets the same load through;
- v2-only and v3-only loads succeed.

## Verification

- **CI gate.** Run locally: `cargo test`, `cargo clippy --all-targets`
  (`-Dwarnings`), `cargo doc` (`-Dwarnings`), `cargo fmt --all --check`
  inside `nix develop`, and kermit-lab's pytest. Run cargo in the foreground
  with `CARGO_BUILD_JOBS=2`. Don't commit a `flake.lock` bump. Toolchain
  drift is resolved at landing.
- **Oracles.** Run `kermit/tests/lubm_mini_oracle.rs`, and
  `kermit/tests/lubm_cardinalities.rs` inside `nix develop`. Confirm that the
  second one actually ran; it skips silently without java.
- **Counts end to end.** Run `bench run … --verify` on a benchmark with
  `expected` counts.
- **q0005.** Build a one-query benchmark whose `expected` is 4169173508,
  over the `watdiv-stress-100-test-1` relations. For each cell, run
  `bench run <it> -q q0005 -i … -a … --metrics space --verify` under
  `/usr/bin/time -v`, fully detached.
  - With space as the only metric, the one join that runs is the untimed
    verify count. The result is a correct count, peak RSS (expected flat,
    not hundreds of GB) and the wall time of one execution.
  - The host is shared with other builds, so wall times are indicative
    only. They go in the hand-off report, not in commits or issues.

## Acceptance (issue #65)

| Criterion | Met by |
|---|---|
| Memory independent of result size, in all three cells | The streaming path, the result-allocation test, and q0005's peak RSS |
| No per-row result allocation in the timed region; `BENCHMARKING.md` accurate | `count` through `join_for_each`, the result-allocation test, the doc rows |
| `--verify` counts without materialising | `family.count` in the verify gate |
| Reports from before and after can't be silently mixed | Schema 3 and the loader's mixing guard |
| `q0005` runs | No policy needed; the verified q0005 run |
| The standard suite exercises the new path | `lftj_join` / `hash_join` route through `_for_each`; `JoinEntry::count` |
