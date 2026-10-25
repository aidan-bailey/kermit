# Bench Runner Measure Layer: One Untimed Build, Exact Space, `bench probe`

**Date:** 2026-10-25
**Status:** Design approved in conversation (2026-10-25), decision by decision and then
section by section.
**Scope:** The binary's bench runners (`kermit/src/bench/`) on the per-query lifecycle of
`2026-10-22-per-query-index-build-design.md`. Each query and cell builds its indexes once
outside Criterion. Every untimed number is read from that one set: the `index` lines, the
space bytes, the verified count, the single-run times. In every cell whose joins leave their
indexes unchanged, `iteration` runs on the same set. A new module holds the measurement order
once for every subcommand. `space` becomes an exact
value in the report instead of a Criterion function. A new `bench probe` subcommand verifies
each query and times one build and one execution, without Criterion. Each run, join and
probe report records its query's plan. The report schema becomes v6.
**Related:**
- `2026-10-22-per-query-index-build-design.md`: the lifecycle (`Catalog`, `prepare`, `build`,
  `execute`) this design measures;
- #99, the sweep: its harness in `kermit-bench-runs/full-sweep-2026-10-24/` is the evidence
  below and the first user of `bench probe`;
- #86, flags that would be silently ignored are usage errors;
- #61, one generic runner per subcommand family;
- `docs/specs/space-benchmarks.md`, which this design supersedes.

## Summary

Today, per query and cell, `run_benchmark` (`kermit/src/bench/run.rs`) builds the query's
indexes up to four times outside any timer:
- for the `index` metadata (`:168`);
- for `--verify` (`:206`);
- for `iteration` (`:308`);
- for `space` (`:362`).

`space` then runs Criterion over a deterministic sum the metadata build already printed.

`bench ds` (`kermit/src/bench/ds.rs`) hand-writes the same pattern and keeps its untimed
relation alive through the `build` and `end_to_end` timers (`:50`), which `bench run`
forbids.

The binary has no single-run measurement, so the sweep's prescreen and side timing run
Criterion `build` (10 samples / 1 s / 1 s) and infer the count time from stderr timestamps.

The reports record each index spec but not the plan's variable order, so nothing can tell
when two optimisers chose the same plan.

This design:
- **One untimed build per query and cell, the describe step.** It is timed by an
  `Instant`, its bytes are read before anything touches it, and it optionally counts the
  answer, also timed. The `index` lines, `space`, verification, the single-run times and
  `iteration` all read this one set.
- **One fixed order in every cell:** describe → `iteration` → drop → `build` →
  `end_to_end`.
- **The order lives in one function**, `Described::measure` in a new
  `kermit/src/bench/measure.rs`. It consumes the describe set, so ownership enforces "no
  other index set alive while a build is timed". `bench run`, `bench join`, `bench probe`
  and `bench ds` all call it.
- **Report v6:** a `values` list beside `criterion_groups`, for the exact `space` and the
  single-run times. The axes gain `plan` and `answer_rows`, and `kind` gains `probe`.
- **`bench probe`:** a front end over the same runner. Per query and cell it verifies
  against `expected`, records the exact `space`, one build time and one execution time.
  Nothing goes through Criterion.

What Criterion times does not change: the bodies of `build`, `iteration` and `end_to_end`
are today's, moved.

## Motivation

The figures come from the 2026-10-24 sweep's prescreen
(`kermit-bench-runs/full-sweep-2026-10-24/prescreen/probe.jsonl`, 1,441 query records at
2026-10-25 11:45). The time-budget model is that sweep's `make_plan.py`, applied to its
default arms. They are estimates of the cost of today's binary, not measurements of this
design.

1. **Untimed builds.**
   - Of about 11 h of default-arm time, 1.5 h is untimed work around Criterion:
     - the metadata and verify builds plus the verify count, 0.38 h;
     - `iteration`'s own build, 0.15 h;
     - `space`'s build, 0.14 h;
     - `space`'s Criterion run, 0.83 h.
   - One build per query and cell, with `space` taken from it, leaves about 0.23 h.
2. **No single run.**
   - The prescreen had spent 5.5 h of `bench run` wall time, about 4.4 h of it Criterion's
     warm-up, sampling and analysis.
   - The work a single run needs, one build plus one count per query, is about 0.8 h.
   - The inferred count (`interval − 2 × build`, clamped at 0) is noisy wherever builds
     dominate, as on JOB. A single run measures it directly.
3. **`space` is exact.**
   - Since v5 every index is built at exact size, and the BuildMode rule pins capacities.
     So a query's `space` is one number, which the describe build knows.
   - `SpaceMeasurement` and its calibration trap exist only to push that number through a
     statistics engine.
4. **Plan identity.**
   - Cost-based and cardinality build identical index sets on 324 of 649 (query, cell)
     pairs in the prescreen: H2O 6/6, LUBM 13/14, SNAP 7/8, WatDiv 60/124, JOB about a
     third.
   - Measuring those once would save at most 2.0 h. Identical index sets don't prove an
     identical plan, and the reports can't say whether the plans were equal.
5. **One methodology, written twice.** `bench ds`'s hand-written sequence already diverged
   from `bench run`'s rule that at most one structure is alive while a build is timed.

## Decisions

1. **One describe step, and one order in every cell:** describe → `iteration` → drop →
   `build` → `end_to_end`.
   - Lazy-HashTrie cells follow the same order, although their `iteration` cannot use the
     describe set.
   - Rejected: `iteration` keeping its own build, so that today's `build` → `iteration`
     order is kept. That costs one build per query and cell, and preserves a comparability
     with v5 that the codegen precision bound has already removed (`BENCHMARKING.md`:
     never compare across binaries).
   - Rejected: `iteration` reusing the last set built inside `build`'s Criterion run. That
     needs a hand-written `iter_custom` loop in the timed path, and it fails when `build`
     is not requested.
2. **Values measured without Criterion go into a `values` list** beside `criterion_groups`.
   - Each entry is `{function, metric, estimator, value}`, and kermit-lab makes one row per
     entry.
   - Rejected: axes. They would put measurements into the fields that are faceted by, and
     `kl.plot(metric="space")` would find no rows.
3. **A shared measure layer** (`Subject`, `Described`, `measure`) used by every runner.
   - Rejected: reordering each runner in place. That keeps two hand-written copies of the
     order and leaves the at-most-one-set rule to comments.
   - Rejected: metric plug-ins with a scheduler. The order is a methodological decision,
     written once rather than computed.
4. **`bench probe` is a front end over the one runner.** `single-run` is not a `bench run`
   metric.
   - Rejected: `-m single-run` on `bench run`, after which a report's `kind` could not tell
     a probe from a measurement.
   - Rejected: offering both spellings.
   - Rejected: a separate probe runner, which would re-split what #61 joined.
   - Rejected: `--single-runs N`. Replicates belong in separate processes, where the sweep
     alternates them.
5. **`bench probe` ignores `bench`'s Criterion flags; it does not reject them.**
   - The flags are `--name`, `--sample-size`, `--measurement-time` and `--warm-up-time`.
   - `BenchArgs` is flattened onto `bench` (`kermit/src/main.rs:401-405`), so `list`,
     `fetch`, `clean` and `gen` already ignore them.
   - #86 exists so that a report never describes a measurement it did not make. An ignored
     Criterion setting changes nothing a probe measures or records.
6. **`bench probe` always verifies** when the benchmark declares `expected`, and a mismatch
   aborts. It has no `--verify` flag.
   - Rejected: opt-in `--verify`, as on `bench run`. A probe exists to screen, and an
     unverified screen is never what is wanted.
7. **The plan axis is the variable order, by name.**
   - For a fixed catalog and query, `prepare` is deterministic up to the optimiser's
     variable order. `plan_indexes` (`kermit-algos/src/index_plan.rs:80`) derives every
     index spec and the executed query from that order.
   - So within one (benchmark, query) the order identifies the whole computation. No hash
     is needed.
8. **`answer_rows` is recorded whenever describe counted, and reports are written after
   every query.** A killed run then keeps every finished query, and a harness can resume
   from the report instead of from stderr.
9. **`bench ds`'s header clone moves inside the `build` timer.** It costs about 100 ns.
   `bench run`'s `build_with` already constructs index headers inside its timer, and an
   untimed setup hook on `Subject` for one clone is not worth it.

## Design

### The measure layer (`kermit/src/bench/measure.rs`, new)

```rust
/// What a bench subcommand measures: something built from flat tuples, then exercised.
pub(crate) trait Subject {
    /// What one build produces: a query's `Indexes<R>`, or one relation.
    type Built;
    /// One build. The timed body of `build`, and of the build half of `end_to_end`.
    fn build(&self) -> Self::Built;
    /// The heap bytes of each built part (each index; the one relation), as built.
    fn heap_bytes(&self, built: &Self::Built) -> Vec<usize>;
    /// One traversal: the answer's rows counted, or the relation scanned. The timed
    /// body of `iteration`, and K times in `end_to_end`.
    fn exercise(&self, built: &Self::Built) -> u64;
    /// Whether `exercise` changes `built` (a lazy HashTrie expands what a join reaches).
    const EXERCISE_MUTATES: bool;
}
```

- **`Described::new(subject, count) -> Described`** is the describe step.
  - It builds once, timing the build with an `Instant`, then reads `heap_bytes`.
  - If `count` is set, it then exercises once and times that too.
  - It holds the built set, the bytes, the build time, and `Option<(rows, exercise
    time)>`.
  - Accessors: `built()`, `bytes()`, `rows()`.
- **`Described::measure(self, request) -> Measured`** runs the requested metrics in the
  fixed order.
  - `iteration`:
    - eager: `b.iter(|| subject.exercise(&built))` on the describe set, after which the
      set is dropped;
    - when `EXERCISE_MUTATES`: the set is dropped first, then `iter_batched` with
      `subject.build()` as untimed setup per sample.
  - `build`: `iter_batched(|| (), |()| subject.build(), PerIteration)`.
  - `end_to_end`: one build, then K exercises, per sample, returned so that Criterion
    frees them outside the timer.
  - These bodies are today's (`run.rs:268-346`, `ds.rs:88-150`), moved.
  - A Criterion instance and group are created only when a time metric is requested.
    `build_time_criterion` moves here.
  - `request` carries:
    - the metrics;
    - `single_run`;
    - `queries_per_build`;
    - `&BenchArgs`;
    - the group name;
    - the function-id prefix: `None` for run, join and probe, `Some(ds)` for `bench ds`.
  - `Measured` holds the `CriterionGroupRef`s and the `ValueEntry`s:
    - `space`, the sum of the bytes, when `space` is requested;
    - when `single_run` is set, `build` with estimator `single`, from describe's build time;
    - when `single_run` is set and describe counted, `iteration` with estimator `single`,
      from describe's exercise time. The runner always counts when `single_run` is set.
- **The subjects:**
  - `QuerySubject { family, catalog, prepared }`: `build` is `family.build_indexes`,
    `heap_bytes` is each index's `heap_size_bytes()`, `exercise` is `family.count_rows`,
    and `EXERCISE_MUTATES` is `F::JOIN_MUTATES`.
  - `RelationSubject { family, header, tuples }`: `build` is
    `family.build_index(header.clone(), TupleView::all(tuples))`, `heap_bytes` is the
    relation's `heap_size_bytes()`, `exercise` is `F::scan`, and `EXERCISE_MUTATES` is
    false. `HashTrie::for_each_tuple` reads an unexpanded child's
    pending chain and expands nothing (`kermit-ds/src/ds/hash_trie/implementation.rs:243`).
- **Deleted:**
  - `kermit/src/measurement.rs`: `SpaceMeasurement` and `BytesFormatter`'s
    `ValueFormatter`. `format_bytes` and its unit scale move to `bench_report.rs`, whose
    metadata blocks use them.
  - From `kermit/src/bench/mod.rs`: `build_space_criterion`, `add_space_bench` and
    `heap_bytes_of`.
  - `run.rs`'s zero-byte skip (`:377-384`) and its second-build check for lazy HashTrie
    (`:364-372`).

### The runners

- **`run.rs`** (`bench run`, `bench join`, `bench probe`). Per query:
  1. Build a `QuerySubject`.
  2. Call `Described::new(&subject, count)`, where `count` is
     `(verify && expected.is_some()) || single_run`.
  3. Assemble the `index` lines from `prepared.indexes()`, `prepared.index_rows()` and
     `described.bytes()`.
  4. Verify, if asked: same comparison, same abort message.
  5. Print the metadata block. The probe titles it `bench probe metadata` and adds a
     `single run` line giving both times; run and join keep their header. Every counted
     query also gets an `answer rows` line.
  6. Call `measure`.
  7. Assemble the report and push it to the sink.

  `run_benchmark` takes the `ReportSink` and pushes after every query, where today the
  handler pushes after every cell (`main.rs:869`).
- **Axes:** run, join and probe reports gain:
  - `plan`: a string, the variable order in the executed query's variable names joined by
    `", "` (for example `"X, Y, Z"`). It comes from a new `Prepared::variable_order()`
    (`kermit/src/db/lifecycle.rs`), which uses `canonical_names(&query)` indexed by
    `plan.variable_ordering`. A `_` placeholder appears under the `K<n>` name
    `rewrite_placeholders` gave it.
  - `answer_rows`: a u64, whenever describe counted.
- **`ds.rs`** (`bench ds`):
  - It builds a `RelationSubject` and calls `Described::new(&subject, false)`.
  - The `tuples` metadata and the optimisation axes are read from `described.built()`.
  - Then it calls `measure`, with the prefix `Some(ds_name)`.
  - Function ids keep their prefix: `{ds}/build`, `{ds}/iteration`, `{ds}/end_to_end`,
    and `{ds}/space` in `values`.
- **`bench join`** runs through `run.rs` as today; its `--output` CSV path is unchanged.
  `kermit join` is unchanged.

### `bench probe` (`kermit/src/main.rs`)

- **`RunSelection`.** `Run`'s inline selection fields (`main.rs:294-362`) move into one
  `#[derive(Args)]` struct: the benchmark name, `--all`, `-q`, `-i`, `-a`, the planner,
  `--force`, and the layout, config and build choices. It is flattened into
  `BenchSubcommand::Run` and the new `BenchSubcommand::Probe`. `bench run`'s command line
  does not change.
- **What `probe` has:** no `--metrics`, `--queries-per-build` or `--verify`. Its help text
  says that `bench`'s Criterion settings and `--name` do not apply. `--report-json` does
  apply.
- **The handler.** It calls `run_bench_run_command`, which gains a `kind` parameter in
  place of its hard-coded `BenchKind::Run`. The probe passes:
  - `kind: Probe`;
  - metrics `[Space]`;
  - `verify: true`;
  - a new `RunSettings.single_run: true`.
- **Group check.** The collision check on Criterion group names
  (`check_sweep_group_directories`) runs only when a time metric is requested. A probe
  writes no groups.
- **Result.** Per query and cell, a probe report holds the exact `space`, single-run `build`
  and `iteration`, `answer_rows`, `plan` and, when `expected` exists, `verified`. Its default
  path is `bench-runs/probe-<millis>.json`.

### Report schema v6 (`kermit/src/bench_report.rs`)

- **`REPORT_SCHEMA_VERSION = 6`.** `space` leaves `criterion_groups`, so a v5 reader finds
  no space function. That is a structural change under the versioning policy. No metric
  changes what it measures.
- **`BenchReport.values: Vec<ValueEntry>`**, always serialised and possibly empty.
  - Each `ValueEntry` is `{function: String, metric: ReportMetric, estimator: Estimator,
    value: u64}`.
  - `Estimator` serialises lower-case: `exact` for a deterministic quantity, `single` for
    one run's wall time.
  - Units follow the metric, as Criterion's do: ns for time, bytes for space.
  - Times come from `Duration::as_nanos()`, saturated at `u64::MAX`.
  - `BenchReport::new` takes the values.
- **`BenchKind::Probe`**, serialised `"probe"`.
- **`ReportMetric::Space`** keeps its wire value `"space"`. Its doc now reads "exact heap
  bytes, reported in `values`".
- **Function ids in `values`:** `space` for run, join and probe; `{ds}/space` for ds; and
  `build` and `iteration` for the probe's single runs. `criterion_groups` is unchanged and
  holds no `space` entry in v6.
- **`docs/specs/bench-report-schema.md`** gains:
  - the v6 changelog row;
  - catalogue entries for `values`, `ValueEntry`, `estimator`, `kind: probe`, `plan` and
    `answer_rows`;
  - a rewritten "Time and space function ids";
  - the null-slope note for `SpaceMeasurement` moved to history (reports up to v5).

### kermit-lab (`python/kermit-lab`)

- **`loader.py`:** a `ValueEntry` dataclass and `BenchReport.values`, empty for reports up
  to v5. `has_metric` also looks at `values`.
- **`frame.py`:**
  - `load()` adds one summary row per value entry and an `estimator` column after `phase`.
    Criterion rows carry `criterion`.
  - A value row puts its value in `mean_ns` and `median_ns`.
    - An `exact` row sets its bounds to the value and `mean_se` to 0, as Criterion's
      deterministic space rows read today.
    - A `single` row has NA bounds and `se`.
  - Value rows have NA `criterion_group` and `criterion_function`.
  - `load_samples` is unchanged.
  - `plan` joins the string axis keys and `answer_rows` the integer ones.
- **Defaults:**
  - `encoding.select` and `resolve_tradeoff` (`encoding.py:67`, `:137`) and
    `speedup_table` (`analysis.py:283`) take time rows with `estimator == "criterion"`
    unless told otherwise.
  - `AestheticMap.estimator` selects otherwise, for example `kl.plot(…,
    estimator="single")`, and the `plot` CLI gains `--estimator`.
  - Space rows are selected whatever their estimator: v5's Criterion space and v6's exact
    space are the same bytes.
  - `compare` already pairs rows on every non-value column, so `estimator` becomes a
    pairing key.
- **Versions:** `SCHEMA_VERSION = 6`, pinned to `REPORT_SCHEMA_VERSION` by the existing
  `cargo test`. There is no `MEANING_CHANGES` entry, so v5 and v6 reports load together.

## Error handling

- **Verification** (`bench run --verify`, and always in `bench probe`):
  - Describe's count is compared with `expected` before any timer.
  - A mismatch aborts with today's message (`verification failed: benchmark '…' query '…'
    on {ds}/{algo} returned N tuples, expected M`).
  - The earlier queries' reports are already on disk, and the failing query writes none.
- **No `expected`:** the probe records `answer_rows` and prints today's "no expected
  cardinality … not verified" note. `bench run --verify` behaves as today.
- **Lazy HashTrie:** describe reads the bytes before its count, so `space` is the as-built
  footprint by construction.
- **Zero heap bytes** (no index, or a TreeTrie whose view keeps no row) record an exact `0`.
- **A killed, timed-out or panicking process** keeps every finished query's report. The
  query in progress leaves nothing.
- **Misuse.** `measure(self)` consumes the describe set, so using it afterwards does not
  compile.
- **`bench probe`** reports the same errors as `bench run` for the same inputs:
  - an incompatible `-i`/`-a` pair, and unreached `--ds-*` flags (#86);
  - an unknown `-q`;
  - spec drift on a `generator:` benchmark without `--force`;
  - a query rejected from the headers (#78).

## Testing

- **`measure.rs` unit tests** use a recording `Subject`.
  - Its built value has an id and a `Drop` that logs. Build, `heap_bytes` and exercise also
    append to the log, and a live counter is checked inside every build.
  - They assert that:
    - describe builds exactly once, reads the bytes before any exercise, and exercises
      exactly when `count` is set;
    - an eager `iteration` exercises the describe set itself;
    - the describe set is dropped before the first timed build, and before the first fresh
      `iteration` build when `EXERCISE_MUTATES`;
    - the live counter never exceeds 1;
    - `space` equals the sum of the bytes, and 0 stays 0;
    - the single-run `build` entry exists exactly when `single_run` is set, and the
      single-run `iteration` entry exactly when describe also counted.
  - Criterion runs with tiny settings into a temp dir, as
    `criterion_directory_name_matches_criterion` (`kermit/src/bench/mod.rs`) does.
- **`run.rs` tests:**
  - the `plan` string;
  - `answer_rows` present exactly when describe counted;
  - a verification mismatch aborts before any Criterion output;
  - a failure on query 2 leaves query 1's report in the file.
- **CLI tests:**
  - A new `kermit/tests/cli_bench_probe.rs`:
    - on a fixture benchmark: `kind: probe`, the three values, `answer_rows`, `plan` and
      `verified`;
    - a wrong `expected` aborts;
    - no Criterion directory is written.
  - The tests that read `space` through `criterion_groups` move to `values`:
    `kermit/tests/common/cli.rs` and the `cli_*` files that use it.
  - `bench run` and `bench ds` reports carry `space` only in `values`.
- **kermit-lab tests:**
  - `test_loader` (`values`, both estimators);
  - `test_frame` (value rows, the `estimator` column, mixed v5 and v6);
  - `test_encoding`, `test_presets` and `test_plot` (the estimator defaults,
    `estimator="single"`);
  - `test_cli` (`--estimator`);
  - `test_contract`, against the real binary: `bench run`'s `space` from `values`, and a
    `bench probe` report.
- **Unchanged and run by the gate:** the join suites, the LUBM and WatDiv oracles,
  `tuple_allocation.rs` and `result_allocation.rs`. The timed bodies do not change.

## Acceptance

1. Outside Criterion, each query and cell is built exactly once, pinned by the
   recording-subject tests.
2. No index set is alive while `build` or `end_to_end` is timed, and an eager `iteration`
   runs on the describe set. The same tests pin this.
3. `space` is an exact value in `values`. `SpaceMeasurement`, `add_space_bench`,
   `build_space_criterion` and `heap_bytes_of` are gone.
4. `bench probe` behaves as specified, always verifying against `expected`
   (`cli_bench_probe.rs`).
5. Reports are v6 with `values`, `estimator`, `plan`, `answer_rows` and `kind: probe`.
   kermit-lab's `SCHEMA_VERSION` is 6, and the contract test passes.
6. The gate passes:
   - `cargo test`;
   - clippy with `-Dwarnings`;
   - `cargo fmt --check`;
   - `cargo doc` with `-Dwarnings`;
   - the python job;
   - `cargo rustdoc -p kermit --bin kermit -- --document-private-items -D warnings`. The
     CI never lints the binary's docs, and `measure.rs` is in the binary.
7. **The order check.** This is a measurement, run after the 2026-10-24 sweep releases the
   host.
   - Within the new binary, `build` under `-m build` matches `build` under `-m iteration
     build`.
   - Coverage, on all three cells:
     - the sweep's WatDiv Criterion set (`sw-wd`, fast builds);
     - the JOB template whose median build is the slowest among those every cell completes
       in that sweep's Criterion set.
   - Method: 5 replicates, alternating which of the two invocations runs first.
     - Per query, the ratio is the Criterion `build` median after `iteration` divided by
       the median alone, as a geometric mean over the replicates.
     - Per cell, report the geometric mean over queries with a bootstrap 95% CI over
       queries.
   - Pass: the CI contains 1, or lies wholly within [0.97, 1.03] (the codegen precision
     bound, `BENCHMARKING.md`).
   - Otherwise stop: the order then matters for that structure, and B's order is
     reconsidered with the owner.
8. The documentation below is updated.

## Documentation

- `CLAUDE.md`:
  - rewrite the "Space benchmarks" gotcha: `space` is an exact report value and the
    calibration trap is gone;
  - update "JSON bench reports" (v6, `values`, `estimator`) and "bench `--name` semantics"
    (`bench probe` ignores it);
  - add a `bench probe` example to the build commands.
- `BENCHMARKING.md`: the per-query order (describe → iteration → build → end_to_end), the
  probe workflow (screening, side timing, `space` from the probe pass), and what
  `criterion`, `exact` and `single` mean. A single run is cold.
- `USAGE.md`: `bench probe`.
- `docs/specs/bench-report-schema.md`: v6, as above.
- `docs/specs/space-benchmarks.md`: a superseded note pointing here.
- `docs/specs/benchmarking-architecture.md` and `ARCHITECTURE.md`: the measure layer.
- kermit-lab's `README.md` and `CHANGELOG.md`.

## Sequencing

One branch, landed in one push with one schema bump, as v5 was. Each phase leaves the gate
green.

1. **kermit-lab reads `values` and `estimator`.** This is additive: it is tested on synthetic
   reports that carry `values`, `SCHEMA_VERSION` stays 5 until phase 2, and the binary does
   not change.
2. **The measure layer and both runners.**
   - `measure.rs` with the two subjects, then `run.rs` and `ds.rs` on it, `space` in
     `values`, the `plan` and `answer_rows` axes, and per-query report writes.
   - Report v6, together with kermit-lab's `SCHEMA_VERSION = 6` and the contract test.
   - Move `format_bytes` to `bench_report.rs`, then delete `measurement.rs` and the space
     helpers.
3. **`bench probe`:** `RunSelection`, `BenchKind::Probe`, `RunSettings.single_run`, and
   `cli_bench_probe.rs`.
4. **Documentation.**

While the sweep holds the host, the work is edits only. Builds, the gate and the order check
wait for its `CHAIN_EXIT`.

## Out of scope

- Sharing one measurement between optimisers that chose the same plan. The `plan` axis
  makes that decidable; the policy is the owner's call.
- The sweep harness in `kermit-bench-runs/`: moving the prescreen, the side timing and
  `space` to `bench probe`, and dropping `-m space` and `--verify` from timed steps.
- Criterion's settings. On WatDiv and LUBM most of each function's roughly 3 s is warm-up,
  minimum measurement and analysis, not the query: WatDiv's median query builds in 10 ms and
  counts in 4 ms.
- Criterion's `base/` copy, which is byte-identical to `new/`.
- A per-query timeout inside the binary.
- A probe for `bench join`'s ad hoc queries.
