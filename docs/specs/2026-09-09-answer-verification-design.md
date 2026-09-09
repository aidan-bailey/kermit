# Answer Verification, Rust–Python Contract, and Download Integrity

**Date:** 2026-09-09
**Status:** Approved design, not yet implemented
**Scope:** Sub-projects C, D and E of the benchmarking-flow improvement
series (A: sweep hardening — landed; B: runner consolidation — landed).
Sections 1–6 are C; Part D and Part E follow. One plan implements all three,
ordered C → D → E because D's contract test exercises C's `verified` axis.

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

---

# Part D — Rust–Python contract

## Motivation

The report schema is held together by two hand-duplicated constants
(`REPORT_SCHEMA_VERSION` in `kermit/src/bench_report.rs`, `SCHEMA_VERSION` in
`python/kermit-lab/kermit_lab/__init__.py`), a hand-maintained axis
include-list in `frame.py` that silently drops unknown keys, and no test that
feeds real binary output into `kermit-lab`. CI never runs the Python tests.
The Criterion 0.8 bump was validated by eye for exactly this reason. Part C
adds a `verified` axis that `frame.py` would drop today.

## D.1 Schema-version pin (Rust side, no Python needed)

A unit test in `kermit/src/bench_report.rs` embeds the Python module at
compile time and asserts the two constants agree:

```rust
const KERMIT_LAB_INIT: &str = include_str!("../../python/kermit-lab/kermit_lab/__init__.py");
// parse the line `SCHEMA_VERSION = <n>`, assert n == REPORT_SCHEMA_VERSION
```

`include_str!` makes the Python file a build input of the test, so the pin
runs in every `cargo test` with no Python toolchain. The message on failure
names both files.

## D.2 `verified` reaches the DataFrame

`frame.py` gains `_AXIS_BOOL_KEYS = ("verified",)`, included in
`_SUMMARY_COLUMNS_CORE` after the int keys and cast with
`.astype("boolean")` (nullable; absent → `<NA>`), mirroring how bool-valued
`ds_config_*` flags are already handled. `bench-report-schema.md`'s table is
the source of truth the comment in `frame.py` already points at.

## D.3 Contract test (real binary → `kermit-lab`)

`python/kermit-lab/tests/test_contract.py`, skipped unless `KERMIT_BIN` names
a built `kermit` binary:

1. `bench ds` on `kermit/tests/fixtures/edge.csv` with `-i tree-trie -m space`,
   run with `cwd` = a temp dir and `--report-json` inside it; then
   `kl.load(report, criterion_root=<tmp>/target/criterion)` yields exactly one
   row with `kind == "ds"`, `metric == "space"`, `data_structure == "TreeTrie"`,
   `mean_ns > 0`.
2. `bench run triangle -i tree-trie -a leapfrog-triejoin -m iteration --verify`,
   run with `cwd` = the workspace root (so discovery finds `benchmarks/` and
   Criterion writes under `<root>/target/criterion`); the loaded frame has one
   row with `benchmark == "triangle"`, `query == "triangle"`, `verified` is
   `True`.

Both cases use `--sample-size 10 --measurement-time 1 --warm-up-time 1`.
This is the only test that would catch a Criterion JSON-layout change or a
Rust-side key rename.

## D.4 CI

A `python` job in both `.github/workflows/pr.yml` and `build.yml`: checkout,
nightly toolchain + `Swatinem/rust-cache`, `cargo build -p kermit`,
`astral-sh/setup-uv` (pinned by commit SHA like the other actions), then in
`python/kermit-lab`: `uv sync --group test` and
`KERMIT_BIN=$GITHUB_WORKSPACE/target/debug/kermit uv run pytest`. The
`triangle` benchmark is committed, so the contract test needs no network.

## D.5 Tests

- Rust: the D.1 pin (and a negative check that the parser rejects a file
  without the line, so a moved constant cannot silently pass).
- Python: `test_frame.py` gains cases for `verified` present (`True`), absent
  (`<NA>`), and dtype `boolean`; `test_contract.py` as above (skips locally
  without `KERMIT_BIN`; the plan runs it once locally with the env var set).

## D.6 Docs

`python/kermit-lab/README.md` (running the contract test; `verified` column),
`CLAUDE.md` (JSON-report gotcha: the schema-version pin and the CI Python
job; CI-checks list), `docs/specs/bench-report-schema.md` (note that
`frame.py` must be extended for new non-`ds_*`/`algo_*` keys — already there —
plus the `verified` row from Part C).

## Out of scope (D)

Making `kermit-lab` a workspace-level check inside `cargo test`; testing
plots; Python packaging or version bumps.

---

# Part E — Download integrity

## Motivation

`kermit-bench/src/cache.rs::download_file` trusts whatever a URL serves and
never re-checks a cached file. A relation that changed upstream, or was
truncated on disk after the atomic rename, is used silently.

## Decision

Verify on download and on `bench fetch`; never on `bench run`. Cold-cache
cost is paid once; hot runs are unchanged; `bench fetch` is the explicit
"make sure my data is intact" command.

## E.1 Schema

`RelationSource` gains `sha256: Option<String>` (`#[serde(default,
skip_serializing_if = "Option::is_none")]`). `validate()` requires, when
present, exactly 64 lowercase hex characters. It is allowed with either
`url` or `path` (a committed file can be pinned too).

`benchmarks/triangle.yml`'s `edge` relation gets the digest of the committed
`benchmarks/data/triangle/edge.csv`. Oxford YAMLs stay unpinned: their URLs
are placeholders, so there is no hosted file to pin. Generator-emitted YAMLs
stay unpinned: their `file://` relations are covered by `meta.json`'s
`spec_hash`.

## E.2 `kermit-bench`

- `pub fn sha256_hex(path: &Path) -> Result<String, BenchError>` (streaming,
  reusing the `sha2` dependency and the existing `hex_digest` helper made
  `pub(crate)`).
- `download_file` gains `expected: Option<&str>`: after writing the `.part`
  file it hashes it; on mismatch it deletes the `.part` and returns
  `BenchError::Integrity { relation, location, expected, actual }` (new
  variant, `Display`: "integrity check failed for relation '…' from …:
  expected sha256 …, got …"). Only on match does it rename into place.
- `pub fn verify_integrity(benchmark, workspace_root) -> Result<usize, BenchError>`
  re-hashes every relation that declares `sha256` (cached `url:` files and
  `path:` files alike) and returns how many it checked; the first mismatch
  is the same `Integrity` error.
- `ensure_cached` passes each relation's `sha256` into `download_file`; it
  does not hash files that already exist.

## E.3 CLI

`run_fetch` calls `ensure_cached` then `verify_integrity` and prints
`  Verified <n> relation(s).` (or `  No integrity hashes declared.` when
`n == 0`). No new flags.

## E.4 Tests

- `kermit-bench` unit: `sha256_hex` on a temp file matches a known vector;
  `validate()` rejects uppercase, short, and non-hex digests and accepts a
  valid one; `verify_integrity` on a temp workspace with a `path:` relation
  passes with the right digest, fails with the `Integrity` error on a wrong
  one, and returns 0 when nothing is declared; the digest check used by
  `download_file` is factored into a pure `check_digest(bytes, expected)`
  tested directly (no network in tests).
- `kermit` CLI (`kermit/tests/cli_bench_fetch_integrity.rs`, fake cache like
  `cli_bench_run_sweep.rs`): a cache-side YAML whose relation declares the
  correct digest of the copied fixture parquet → `bench fetch <name>` exits 0
  and prints `Verified 1 relation(s)`; a wrong digest → non-zero exit and
  stderr contains `integrity check failed` with both digests.
- Gate: `bench fetch triangle` prints `Verified 1 relation(s)`.

## E.5 Docs

`benchmarks/README.md` (schema: `sha256`, how to compute it with
`sha256sum`), `USAGE.md` (`bench fetch` verifies), `CLAUDE.md` (cache gotcha:
verification policy), `docs/specs/benchmarking-architecture.md` (`bench fetch`).

## Out of scope (E)

Hashing on `bench run`; a flag to print digests for unpinned relations;
pinning oxford or generated benchmarks; any change to `kermit-rdf`.
