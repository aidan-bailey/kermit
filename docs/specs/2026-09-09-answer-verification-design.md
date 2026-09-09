# Answer Verification

**Date:** 2026-09-09
**Status:** Approved design, not yet implemented
**Scope:** Sub-project C of the benchmarking-flow improvement series
(A: sweep hardening — landed; B: runner consolidation — landed;
C: answer verification; D: Rust–Python contract; E: download integrity).

## Motivation

`bench run` times joins and discards their results. Nothing in the flow
checks that a timed query produced the right answer: the only oracle is the
`lubm_cardinalities.rs` integration test, which needs a JDK and never runs
in CI. The 2026-09-08 failed-descent bug in LFTJ produced plausible timings
for weeks. Meanwhile the LUBM generator already writes the paper's reference
cardinalities as `expected/q*.csv` sidecars that nothing reads, and static
benchmarks such as `triangle.yml` state their answer count only in prose.

A second latent problem: those LUBM sidecars are emitted whenever
`scale == 1`, but the paper's Table 3 values hold only for LUBM(1, 0) — seed
0, start index 0. A `--seed 5` run at scale 1 would inherit wrong expectations.

## Decisions taken during brainstorming

- **Expected cardinality lives in the YAML query definition**, as an
  optional `expected: <count>` on `QueryDefinition`. Generators write it into
  the cache-side `benchmark.yml`; static benchmarks set it by hand. The
  `expected/*.csv` sidecars are dropped.
- **Verification is opt-in (`--verify`) and a mismatch aborts the run.**
  With the flag, each query that has `expected` is run once before timing
  and its tuple count compared; a mismatch is a hard error. Queries without
  `expected` are skipped with a stderr note. Without the flag nothing changes.

## 1. Schema

`kermit-bench/src/definition.rs`:

```rust
pub struct QueryDefinition {
    pub name: String,
    pub description: String,
    pub query: String,
    /// Expected number of result tuples, when known. Read by
    /// `bench run --verify`; absent means "unknown", not "zero".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<u64>,
}
```

No validation rule: `expected: 0` is legitimate (empty-result queries).
Every existing YAML parses unchanged; the key is written only when present.

`kermit/src/bench/workload.rs`: `NamedQuery` gains `pub expected: Option<u64>`,
copied from the definition by `Workload::from_definition`; `Workload::adhoc`
sets `None`.

`benchmarks/triangle.yml`: `expected: 4` (the description already says four
matches; the plan verifies it by running the query). Oxford YAMLs stay
unset — their counts are not known.

## 2. Generators (`kermit-rdf`)

- `generator::Generator::translate_queries` returns
  `Vec<TranslatedQuery>` instead of `Vec<(String, String)>`:
  ```rust
  pub struct TranslatedQuery {
      pub name: String,
      pub datalog: String,
      pub expected: Option<u64>,
  }
  ```
  `yaml_emit::YamlInputs.queries` takes the same type and writes `expected`
  into each `QueryDefinition`. `collect_used_predicates` reads `.datalog`.
- LUBM: `translate_queries` fills `expected` from `LubmQuerySpec::expected_cardinality`.
  WatDiv (stress and basic): `None`.
- The `Generator::write_expected` hook, the `expected/` directory in
  `process_artifacts`, the `expected` module (`write_cardinality_csv`,
  `parse_desc`, `write_expected_csvs`) are removed; no `meta.json` field
  refers to them. `process_artifacts` stages become A–F. The toy generator in `generator.rs` tests and the e2e tests drop
  their `expected/` assertions.
- The LUBM reference gate becomes `scale == 1 && seed == 0 && start_index == 0`
  at both call sites (`kermit/src/materialize.rs::run_lubm` and
  `kermit/src/main.rs::run_gen_lubm`), with a unit test on a small pure
  helper `lubm_reference_applies(scale, seed, start_index) -> bool` in
  `kermit-rdf/src/lubm/queries.rs`.

## 3. Runner (`kermit`)

- `bench run` gains `--verify` (bool flag, long-only). `bench join` does not
  (an ad-hoc workload has no `expected` source).
- `RunSettings` gains `pub verify: bool`.
- In `run_benchmark`, inside the per-query loop and before any Criterion
  group is opened:
  ```rust
  let verified = if settings.verify {
      match query_def.expected {
          | Some(expected) => {
              let actual = family.join(&engine, query_def.query.clone()).len() as u64;
              if actual != expected {
                  anyhow::bail!(
                      "verification failed: benchmark '{}' query '{}' on {}/{} returned {} \
                       tuples, expected {}",
                      workload.name, query_def.name, ds_name, algo_name, actual, expected
                  );
              }
              true
          },
          | None => {
              eprintln!(
                  "bench run: no expected cardinality for query '{}' in benchmark '{}'; \
                   not verified",
                  query_def.name, workload.name
              );
              false
          },
      }
  } else {
      false
  };
  ```
  and after the axes map is built: `if verified { axes.insert("verified", json!(true)); }`.
  The error propagates through `dispatch_run_bench` to `run_bench_run_command`,
  whose existing per-cell context adds the retained-report path; the sink
  already holds every earlier cell.
- The metadata block gains `verified: yes` only when the check passed, so
  stderr and the report agree.

## 4. Unchanged without `--verify`

No extra query execution, no `verified` axis, no metadata line; report bytes
identical to today. `schema_version` stays 2: `verified` is an additive
boolean key, present only after a passed check.

## 5. Tests

- `kermit-bench`: a definition with `expected: 7` round-trips through YAML;
  one without serialises with no `expected` key; a legacy YAML without the
  key deserialises to `None`.
- `kermit-rdf`: `yaml_emit` writes `expected` when set and omits it
  otherwise; `lubm_reference_applies` is true only for `(1, 0, 0)`; the
  toy-generator test and `e2e_watdiv` / `e2e_lubm` / `lubm_pipeline` no longer
  expect an `expected/` directory (and assert it is absent).
- `kermit` unit: `Workload::from_definition` carries `expected` per query;
  `adhoc` yields `None`.
- `kermit` CLI (`kermit/tests/cli_bench_run_verify.rs`, on a fake cache built
  like `cli_bench_run_sweep.rs`, `--metrics iteration`, tree-trie):
  1. correct `expected` + `--verify` → success, report axis `verified: true`,
     stderr metadata contains `verified`;
  2. wrong `expected` + `--verify` → non-zero exit, stderr contains
     `verification failed` with both counts, and — with two benchmarks in the
     cache where the first is correct — the report file holds the first
     benchmark's reports (the sink contract);
  3. no `expected` + `--verify` → success, stderr contains `not verified`,
     no `verified` axis;
  4. correct `expected` without `--verify` → no `verified` axis.
  The wrong-count case is a built-in mutation check: the test passes only if
  the comparison actually runs.
- Gate: `bench run triangle --verify -m iteration` succeeds for all three
  cells.

## 6. Docs

`benchmarks/README.md` (schema: `expected`), `USAGE.md` (`--verify`),
`BENCHMARKING.md` (a "Verify before you trust" paragraph: run with `--verify`
once per cell before a measurement campaign), `CLAUDE.md` (JSON-report
gotcha: `verified` axis; LUBM gotcha: reference gate is LUBM(1, 0); a line in
the `kermit-rdf` architecture entry replacing `expected` in the shared-stage
list), `docs/benchmarks/LUBM.md` and `WATDIV.md` (no sidecars; `expected` in
the YAML), `docs/specs/bench-report-schema.md` (`verified` row),
`docs/specs/benchmarking-architecture.md` (`bench run` flow step).

## Out of scope

- Comparing full result sets, not just counts.
- `--expected N` on `bench join`.
- Filling in `expected` for the oxford benchmarks or the committed WatDiv
  snapshots (their Python-era `expected.json` values could be folded in
  later by the preprocessor).
- Rewriting `lubm_cardinalities.rs` on top of `--verify`.
- Verification in `bench ds` (no query).

## Verification

Full gate inside `nix develop` (fmt check, clippy `-Dwarnings`, doc
`-Dwarnings`, `cargo test`), plus `bench run triangle -i all -a all -m
iteration --verify` (three passes, three `verified: true` axes) and a fake
`expected: 5` on a scratch copy of `triangle.yml` under `KERMIT_WORKSPACE`
to see the failure message by eye.
