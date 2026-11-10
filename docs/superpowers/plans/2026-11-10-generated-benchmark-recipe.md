# Generated-Benchmark Recipe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every generated-benchmark kind one spec type (its YAML block and its `bench gen` flag set) and one `BenchmarkKind` impl shared by `bench gen` and `materialize`, verify every generated benchmark on every cell and optimiser through one hook, and write the recipe for adding a kind into CLAUDE.md.

**Architecture:** `kermit-bench`'s `GeneratorSpec` keeps its six variants, each now wrapping a per-kind struct that derives `clap::Args` behind an optional `clap` feature, with every committed spec hash pinned first. The binary gains `kermit/src/kinds/`, a `BenchmarkKind` trait with one impl per kind, and two generic drivers (`bench::gen::gen` and `materialize::regenerate`) that replace the per-kind functions of `gen.rs` and `materialize.rs`, kind by kind. A test hook runs `bench probe -i all -a all` under every optimiser over a generated cache, and the end-to-end generator tests move onto it.

**Tech Stack:** Rust (nightly, edition 2021), serde / serde_yaml, clap 4 (derive, env), cargo integration tests through the `kermit` binary.

**Spec:** `docs/specs/2026-11-10-generated-benchmark-recipe-design.md`

---

## File structure

| File | Responsibility | Story |
|---|---|---|
| `kermit-bench/tests/spec_hashes.rs` (new) | Pins the `spec_hash` of every committed generator spec and of each kind's omitted-defaults form | S1 |
| `kermit-bench/src/definition.rs` | `GeneratorSpec` over the six per-kind spec structs; each struct's `validate`; clap derives behind the feature | S2, S3 |
| `kermit-bench/src/lib.rs` | Re-exports the six spec structs | S2 |
| `kermit-bench/Cargo.toml`, `kermit/Cargo.toml`, `Cargo.lock` | The optional `clap` dependency and feature of `kermit-bench`, enabled by the binary | S3 |
| `kermit/src/kinds/mod.rs` (new) | `BenchmarkKind`, `NoTools`, `tools_from_env`, the marker re-exports; the cross-kind unit tests | S4–S8, S11 |
| `kermit/src/kinds/{h2o,snap,job,lubm,watdiv}.rs` (new) | One kind's naming, tools, preflight and generation, once (`watdiv.rs` holds `Watdiv` and `WatdivBasic`) | S4–S8 |
| `kermit/src/main.rs` | `mod kinds;`; its tests' spec constructions | S2, S4 |
| `kermit/src/bench/gen.rs` | `GenSubcommand` (spec, tools, target per kind), `GenTarget`, the generic `gen::<K>` driver | S2–S8 |
| `kermit/src/materialize.rs` | Cache classification (unchanged), `dispatch`, `regenerate::<K>`, `kind_name` | S2, S4–S8 |
| `kermit/tests/common/generated.rs` (new), `kermit/tests/common/mod.rs` | The shared hook `assert_every_cell_answers_expected` | S9 |
| `kermit/tests/{snap,h2o,job}_generate.rs`, `kermit/tests/lubm_mini_oracle.rs` | Moved onto the hook; their own kind-specific checks stay | S2, S9, S10 |
| `kermit/tests/cli_snap_gen.rs`, `kermit/tests/cli_gen_scale_range.rs` | The summary-line assertion; `watdiv-basic`'s scale range | S5, S8 |
| `kermit/tests/cli_gen_kinds.rs` (new) | Decision 9: `--force` with a missing tool keeps the drifted cache | S11 |
| `CLAUDE.md`, `benchmarks/README.md`, `docs/benchmarks/{README,WATDIV,LUBM}.md`, `docs/specs/benchmarking-architecture.md` | The recipe and the documentation the spec lists | S12 |

## Names contract

Names one story defines and a later one uses. The design session wrote these; after it, only `/flow:draft`'s manager edits them.

**Spec structs** (`kermit-bench/src/definition.rs`, S2; re-exported from the crate root). Each derives `Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize`, keeps today's field order and today's serde attributes verbatim, has `pub fn validate(&self) -> Result<(), String>` (the reason only; `validate_generator` wraps it as `BenchError::Invalid { name, reason }`) and `impl From<Self> for GeneratorSpec`:

- `pub struct WatdivSpec { pub scale: u32, pub stress: WatdivStressSpec }` (`#[serde(default)]` on `stress`, as today)
- `pub struct WatdivBasicSpec { pub scale: u32 }`
- `pub struct LubmSpec { pub scale: u32, pub seed: u32, pub threads: u32, pub start_index: u32, pub ontology: String, pub queries: Option<Vec<String>> }`
- `pub struct SnapSpec { pub graph: Option<SnapGraph>, pub url: Option<String>, pub sha256: Option<String>, pub queries: Option<Vec<SnapQuery>> }`
- `pub struct H2oSpec { pub scale: u32, pub seed: u32, pub order: H2oOrder }`
- `pub struct JobSpec { pub template: u8 }`
- `pub enum GeneratorSpec { Watdiv(WatdivSpec), WatdivBasic(WatdivBasicSpec), Lubm(LubmSpec), Snap(SnapSpec), H2o(H2oSpec), Job(JobSpec) }`, with today's `#[serde(tag = "kind", rename_all = "kebab-case")]`.

**The `clap` feature** (S3): `kermit-bench/Cargo.toml` `[features] clap = ["dep:clap"]`, `clap = { version = "4.4", optional = true, features = ["derive", "env"] }`; `kermit/Cargo.toml` depends on `kermit-bench` with `features = ["clap"]`. Under it, each spec struct derives `clap::Args`, `WatdivStressSpec` derives `clap::Args` (flattened into `WatdivSpec` with `#[command(flatten)]`), and `SnapGraph`, `SnapQuery`, `H2oOrder` derive `clap::ValueEnum`. Flag spellings are today's: `--scale`, `--max-query-size`, `--query-count`, `--constants-per-query`, `--allow-join-vertex`, `--seed`, `--threads`, `--start-index`, `--ontology`, `--queries` (comma-delimited, on LUBM and SNAP), `--graph`, `--url`, `--sha256`, `--order`, `--template`. Every clap default equals its serde default. `--template` takes `clap::value_parser!(u8).range(1..=33)`; its error must contain `--template` and `1..=33` (`cli_job_gen.rs:105`). `--scale` keeps `range(1..)` for WatDiv, WatDiv-basic and LUBM and `range(H2O_MIN_SCALE..)` for H2O, so `--scale 0` stays a clap usage error, exit 2 (`cli_gen_scale_range.rs`).

**Kind layer** (`kermit/src/kinds/mod.rs`, S4):

- `pub(crate) trait BenchmarkKind { const KIND: &'static str; type Spec: Into<GeneratorSpec> + Clone; type Tools: clap::Args + clap::FromArgMatches; type Ready; fn bench_name(spec: &Self::Spec, tag: &str) -> anyhow::Result<String>; fn preflight(spec: &Self::Spec, tools: Self::Tools) -> anyhow::Result<Self::Ready>; fn generate(spec: &Self::Spec, ready: &Self::Ready, target: kermit_rdf::generator::Target<'_>) -> anyhow::Result<String>; }`, with the doc comments of the spec's "The kind layer".
- `#[derive(clap::Args, Debug, Clone, Copy, Default)] pub(crate) struct NoTools {}`.
- `pub(crate) fn tools_from_env<T: clap::Args + clap::FromArgMatches>() -> anyhow::Result<T>`: parses `T` from an empty argument list through `T::augment_args(clap::Command::new("kermit"))`.
- Marker unit structs, one per kind, re-exported from `kinds`: `H2o` (S4, `kinds/h2o.rs`), `Snap` (S5, `kinds/snap.rs`), `Job` (S6, `kinds/job.rs`), `Lubm` (S7, `kinds/lubm.rs`), `Watdiv` and `WatdivBasic` (S8, `kinds/watdiv.rs`). `KIND` values: `"h2o"`, `"snap"`, `"job"`, `"lubm"`, `"watdiv"`, `"watdiv-basic"`.
- Per kind, `Tools` / `Ready` / `bench_name`: `H2o`: `NoTools` / `kermit_generate::h2o::KeyPlan` / `h2o-{scale}-{tag}`. `Snap`: `NoTools` / `kermit_generate::snap::SnapSource` / `snap-{graph name or URL stem}-{tag}`. `Job`: `NoTools` / `kermit_generate::job::duckdb::Duckdb` / `job-{template}-{tag}`. `Lubm`: `LubmTools { lubm_jar: Option<PathBuf> }` (`#[arg(long, value_name = "PATH", env = "KERMIT_LUBM_JAR")]`) / `LubmReady { jar: PathBuf, queries: Vec<kermit_rdf::lubm::pipeline::LubmQuerySpec> }` / `lubm-{scale}-{tag}`. `Watdiv` and `WatdivBasic`: `WatdivTools { watdiv_bin: Option<PathBuf>, no_bwrap: bool }` (`#[arg(long, value_name = "PATH", env = "KERMIT_WATDIV_BIN")]`; `#[arg(long, env = "KERMIT_NO_BWRAP", value_parser = clap::builder::FalseyValueParser::new())]`) / `WatdivReady { bin: PathBuf, vendor: PathBuf, use_bwrap: bool }` / `watdiv-stress-{scale}-{tag}`, `watdiv-basic-{scale}-{tag}`.

**`bench gen`** (`kermit/src/bench/gen.rs`, S4 onward):

- `fn gen<K: BenchmarkKind>(spec: K::Spec, tools: K::Tools, target: GenTarget) -> anyhow::Result<()>`, running the spec's seven steps in order.
- `GenTarget::refuse_workspace_name(&self, bench_name: &str) -> anyhow::Result<()>`, split out of `GenTarget::prepare`, keeping its message `--tag {tag:?} produces bench name {bench_name:?} which already exists in the workspace; pick a different tag`; `GenTarget::prepare(&self, kind: &str, bench_name: &str) -> anyhow::Result<PathBuf>` keeps the notes and the directory creation.
- Each `GenSubcommand` variant (`Watdiv`, `WatdivBasic`, `Lubm`, `Snap`, `H2o`, `Job`) is `{ #[command(flatten)] spec: <Kind>Spec, #[command(flatten)] tools: <Tools>, #[command(flatten)] target: GenTarget }`.
- The summary line, matched by tests: `[gen {kind}] wrote {dir} ({summary})` (`cli_job_gen.rs:200` matches `[gen job] wrote`; `cli_snap_gen.rs:251` becomes `[gen snap] wrote` in S5).
- The binary test `required_flags_parse_as_required_yaml_keys` (S3: `snap`, `h2o`, `job`; S7 adds `lubm`; S8 adds `watdiv`, `watdiv-basic`) and its helper `fn parse_gen(args: &[&str]) -> GeneratorSpec`, in `gen.rs`'s tests module.

**`materialize`** (`kermit/src/materialize.rs`, S4 onward):

- `fn dispatch(spec: &GeneratorSpec, clear: &dyn Fn() -> std::io::Result<()>, bench_name: &str, out_dir: &Path, spec_hash: &str) -> anyhow::Result<()>`. `clear` removes a drifted or outdated cache when `materialize` decided to regenerate one (if it exists), then creates `out_dir`. An arm not yet moved to a kind calls `clear()` and then its old `run_<kind>`; S8 removes the last such arm.
- `fn regenerate<K: BenchmarkKind>(spec: &K::Spec, clear: &dyn Fn() -> std::io::Result<()>, bench_name: &str, out_dir: &Path, spec_hash: &str) -> anyhow::Result<()>`: `tools_from_env::<K::Tools>()`, `K::preflight`, `clear()`, then `K::generate` with `Target { out_dir, bench_name, tag: bench_name, spec_hash: Some(spec_hash) }`.
- `pub(crate) fn kind_name(spec: &GeneratorSpec) -> &'static str`, an exhaustive match returning each arm's `K::KIND`.

**Preflight messages** (unit- and CLI-tested): a missing WatDiv binary names `--watdiv-bin` and `KERMIT_WATDIV_BIN`; a missing LUBM jar keeps today's message, which names `--lubm-jar / KERMIT_LUBM_JAR`; a missing DuckDB keeps `Duckdb::locate`'s message, which names `KERMIT_DUCKDB_BIN` (`cli_job_gen.rs:142`); an unknown LUBM query is `unknown LUBM query: {name}`.

**Tests:**

- `kermit-bench/tests/spec_hashes.rs` (S1): `committed_specs_keep_their_hashes`, `omitted_defaults_hash_as_written_defaults`, and `#[ignore] print_committed_spec_hashes`, which prints the table in the test's source format.
- `kermit/tests/common/generated.rs` (S9), registered in `common/mod.rs` under `#[allow(dead_code)]`: `pub fn bench_dir(cache_home: &Path, bench: &str) -> PathBuf` (`<cache_home>/kermit/benchmarks/<bench>`) and `pub fn assert_every_cell_answers_expected(cache_home: &Path, bench: &str)`. It reuses `common::cli::{kermit_bin, reports_of}`.
- `kermit/src/kinds/mod.rs` tests (S11): `kinds_agree_across_yaml_cli_and_dispatch`, `tool_flags_name_their_environment_variables`.
- `kermit/tests/cli_gen_kinds.rs` (S11): `force_with_a_missing_lubm_jar_keeps_the_drifted_cache`, `force_with_a_missing_watdiv_binary_keeps_the_drifted_cache`.

## Stories

- **Story 1** — Pin every committed spec hash.

  Adds `kermit-bench/tests/spec_hashes.rs` on today's code, before any refactor. `committed_specs_keep_their_hashes` reads every `benchmarks/*.yml` (workspace root = `CARGO_MANIFEST_DIR`'s parent) that has a `generator:` block, asserts there are exactly 42, and compares each `GeneratorSpec::spec_hash` with a recorded table, failing with the file and both hashes. `omitted_defaults_hash_as_written_defaults` checks, per kind, that a YAML spec with only its required keys hashes as the same spec with every default written out, and records that hash too. The table is not invented: the developer writes the ignored `print_committed_spec_hashes`, runs it on today's code (`cargo test -p kermit-bench --test spec_hashes -- --ignored --nocapture`), and pastes its output into the table. The pin must be green before Story 2 starts.

  - **Developer:** junior-dev
  - **Files:** `kermit-bench/tests/spec_hashes.rs`
  - **Interfaces:** exposes the pin every later story keeps green; uses `kermit_bench::{BenchmarkDefinition, GeneratorSpec}` and `GeneratorSpec::spec_hash` as they are today
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 cargo test -p kermit-bench --test spec_hashes`
  - **Review:** deep (load-bearing for every cache; a vacuous pin would pass silently)
  - **Reading:** spec "Spec hashes", "Premises (checked 2026-11-10)"; `kermit-bench/src/definition.rs:54-150,318-330`; `kermit-bench/src/lib.rs`

- **Story 2** — Wrap each kind's fields in a spec struct.

  Turns `GeneratorSpec`'s six struct variants into newtype variants over `WatdivSpec`, `WatdivBasicSpec`, `LubmSpec`, `SnapSpec`, `H2oSpec` and `JobSpec`, with the fields, their order and their serde attributes moved verbatim, so every spec serializes and hashes as before (Story 1's pin stays green). Each struct gets `validate`, the body of its `validate_generator` arm returning the reason, and a `From` impl; `validate_generator` becomes one line per kind. Every construction and match of a struct variant moves to the new syntax: `definition.rs`'s own tests, `materialize.rs`, `bench/gen.rs`, `main.rs`'s tests, `h2o_generate.rs` and `snap_generate.rs`. No behaviour changes; no clap yet.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-bench/src/definition.rs`, `kermit-bench/src/lib.rs`, `kermit/src/materialize.rs`, `kermit/src/bench/gen.rs`, `kermit/src/main.rs`, `kermit/tests/h2o_generate.rs`, `kermit/tests/snap_generate.rs`
  - **Interfaces:** exposes the six spec structs, their `validate` and `From` impls, re-exported from `kermit_bench`; uses Story 1's pin
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 bash -c 'cargo test -p kermit-bench && cargo test -p kermit --no-run'`
  - **Review:** deep (the hash-preserving refactor the whole design rests on)
  - **Reading:** spec "Spec types", "Spec hashes", names contract "Spec structs"; `kermit-bench/src/definition.rs:1-330,596-770`; `kermit/src/materialize.rs:270-330`

- **Story 3** — The spec is the flag set for SNAP, H2O and JOB.

  Adds `kermit-bench`'s optional `clap` feature, enabled by the binary, and derives `clap::Args` on all six spec structs and `WatdivStressSpec` and `clap::ValueEnum` on `SnapGraph`, `SnapQuery` and `H2oOrder`, with the attributes the names contract lists. `GenSubcommand::{Snap, H2o, Job}` flatten their spec struct beside `GenTarget` (their `tools` field arrives with their kind story); `run_gen_snap`, `run_gen_h2o` and `run_gen_job` take the spec struct. `SnapGraphArg`, `SnapQueryArg`, `H2oOrderArg`, their `From` impls and their name-pinning tests are deleted. `required_flags_parse_as_required_yaml_keys` gets its `snap`, `h2o` and `job` cases. WatDiv's and LUBM's subcommands are untouched until Stories 7 and 8. `kermit-bench` must still build, lint and document without the feature.

  - **Developer:** intermediate-dev
  - **Files:** `kermit-bench/Cargo.toml`, `kermit-bench/src/definition.rs`, `kermit/Cargo.toml`, `Cargo.lock`, `kermit/src/bench/gen.rs`
  - **Interfaces:** exposes the clap derives and `parse_gen`; uses Story 2's spec structs
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 bash -c 'cargo clippy -p kermit-bench --all-targets -- -D warnings && RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps -p kermit-bench && cargo test -p kermit --bin kermit gen && cargo test -p kermit --test cli_snap_gen --test cli_job_gen --test cli_gen_scale_range'`
  - **Review:** deep
  - **Reading:** spec "The `clap` feature", Decision 4, names contract "The `clap` feature"; `kermit/src/bench/gen.rs:1-360,700-922`; `kermit/tests/cli_job_gen.rs:95-130`; `kermit/tests/cli_gen_scale_range.rs`

- **Story 4** — The kind layer, with H2O as its first kind.

  Creates `kermit/src/kinds/mod.rs` (the trait, `NoTools`, `tools_from_env`) and `kermit/src/kinds/h2o.rs`, and registers `mod kinds;` in `main.rs`. Adds the generic `gen::<K>` driver (splitting `GenTarget::refuse_workspace_name` out of `prepare`) and, in `materialize.rs`, `dispatch`'s `clear` parameter, `regenerate::<K>` and `kind_name`: `materialize` no longer removes a drifted or outdated cache itself, it passes a `clear` that does, so a kind's `preflight` runs before the removal (Decision 9). The arms not yet moved call `clear()` and then their old function. H2O's `GenSubcommand` variant gains `tools: NoTools`, its `run` and `dispatch` arms call the drivers, and `run_h2o` and `run_gen_h2o` are deleted. `bench gen h2o`'s summary becomes `[gen h2o] wrote …`. Unit tests in `kinds/h2o.rs`: its `bench_name`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/kinds/mod.rs`, `kermit/src/kinds/h2o.rs`, `kermit/src/main.rs`, `kermit/src/bench/gen.rs`, `kermit/src/materialize.rs`
  - **Interfaces:** exposes `BenchmarkKind`, `NoTools`, `tools_from_env`, `gen::<K>`, `regenerate::<K>`, `dispatch`'s `clear`, `kind_name`, `GenTarget::refuse_workspace_name`; uses Stories 2–3's `H2oSpec` and its clap derive
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 bash -c 'cargo test -p kermit --bin kermit && cargo test -p kermit --test h2o_generate --test cli_gen_scale_range'`
  - **Review:** deep (the drivers and the reordered cache removal every later kind uses)
  - **Reading:** spec "The kind layer", "`bench gen`", "`materialize`", Decisions 8, 9, 11; names contract "Kind layer", "`bench gen`", "`materialize`"; `kermit/src/materialize.rs:180-330,476-500`; `kermit/src/bench/gen.rs:17-64,600-650`

- **Story 5** — SNAP onto the kind layer.

  Adds `kermit/src/kinds/snap.rs`: `bench_name` (today's `snap_bench_name`, whose tests move here), `preflight` resolving the `SnapSource`, `generate` calling `kermit_generate::run` with the summary `relations=…, queries=…`. SNAP's `GenSubcommand` variant gains `tools: NoTools`; its `run` and `dispatch` arms call the drivers; `run_snap`, `run_gen_snap` and `snap_bench_name` are deleted. `cli_snap_gen.rs:251`'s `snap benchmark written to` becomes `[gen snap] wrote` (Decision 11).

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/kinds/snap.rs`, `kermit/src/kinds/mod.rs`, `kermit/src/bench/gen.rs`, `kermit/src/materialize.rs`, `kermit/tests/cli_snap_gen.rs`
  - **Interfaces:** exposes the `Snap` marker; uses Story 4's trait and drivers
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 bash -c 'cargo test -p kermit --bin kermit && cargo test -p kermit --test cli_snap_gen --test snap_generate'`
  - **Review:** light (moves an existing kind onto Story 4's reviewed drivers)
  - **Reading:** spec "The kind layer" (its table), Decision 11; `kermit/src/materialize.rs:456-480`; `kermit/src/bench/gen.rs:520-600,760-922`; `kermit/tests/cli_snap_gen.rs:240-255`

- **Story 6** — JOB onto the kind layer.

  Adds `kermit/src/kinds/job.rs`: `preflight` is `Duckdb::locate()` (it reads `KERMIT_DUCKDB_BIN`, then `PATH`; no flag is added), and `generate` runs `raw::ensure` on `raw::default_raw_dir()`, then `kermit_generate::run`, with the summary `relations=…, queries=…`. The template range is now clap's (Story 3), so `run_gen_job`'s `TEMPLATES` check goes with it. JOB's `GenSubcommand` variant gains `tools: NoTools`; its `run` and `dispatch` arms call the drivers; `run_job` and `run_gen_job` are deleted. `cli_job_gen.rs`'s order holds: a bad template or tag fails before DuckDB is looked for, and a missing DuckDB fails before the output directory is made.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/kinds/job.rs`, `kermit/src/kinds/mod.rs`, `kermit/src/bench/gen.rs`, `kermit/src/materialize.rs`
  - **Interfaces:** exposes the `Job` marker; uses Story 4's trait and drivers
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 bash -c 'cargo test -p kermit --bin kermit && cargo test -p kermit --test cli_job_gen --test job_generate'`
  - **Review:** light (moves an existing kind onto Story 4's reviewed drivers; `cli_job_gen.rs` pins the order)
  - **Reading:** spec "The kind layer" (its table and the `raw::ensure` paragraph); `kermit/src/materialize.rs:500-520,790-850`; `kermit/src/bench/gen.rs:650-700`; `kermit/tests/cli_job_gen.rs:1-215`

- **Story 7** — LUBM onto the kind layer, with `--queries`.

  Adds `kermit/src/kinds/lubm.rs` with `LubmTools` and `LubmReady`: `preflight` resolves the jar (flag, then `KERMIT_LUBM_JAR`, then `vendored_lubm_jar()`; missing is today's error) and selects the queries (`lubm_query_specs(lubm_reference_applies(…))`, filtered by `spec.queries`, an unknown name being `unknown LUBM query: {name}`); `generate` runs `run_lubm_pipeline` with today's summary fields. LUBM's `GenSubcommand` variant becomes spec, tools and target, which adds `--queries` (Decision 6) and replaces its own `--tag` / `--output-dir` with `GenTarget`'s, same spellings. Its `run` and `dispatch` arms call the drivers; `run_lubm` and `run_gen_lubm` are deleted. `required_flags_parse_as_required_yaml_keys` gains its `lubm` case.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/kinds/lubm.rs`, `kermit/src/kinds/mod.rs`, `kermit/src/bench/gen.rs`, `kermit/src/materialize.rs`
  - **Interfaces:** exposes the `Lubm` marker, `LubmTools`; uses Story 4's trait and drivers and Story 3's `parse_gen`
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 bash -c 'cargo test -p kermit --bin kermit && cargo test -p kermit --test lubm_mini_oracle --test lubm_cardinalities'`
  - **Review:** deep (a new flag, and the tool fallback order on both paths)
  - **Reading:** spec "The kind layer", Decisions 6, 8; names contract "Kind layer" (LUBM), "Preflight messages"; `kermit/src/materialize.rs:400-456`; `kermit/src/bench/gen.rs:186-235,435-518`

- **Story 8** — WatDiv and WatDiv-basic onto the kind layer.

  Adds `kermit/src/kinds/watdiv.rs` with `WatdivTools` (`KERMIT_NO_BWRAP` through `FalseyValueParser`, Decision 7) and `WatdivReady`, shared by the `Watdiv` and `WatdivBasic` markers: `preflight` resolves the binary (flag, then `KERMIT_WATDIV_BIN`, then the vendored path; missing names `--watdiv-bin` and `KERMIT_WATDIV_BIN`), `generate` runs `run_pipeline` or `run_basic_pipeline` with today's driver inputs and summary fields. `GenSubcommand::Watdiv` becomes spec, tools and target (validation and the non-empty note now run, Decision 8), and `GenSubcommand::WatdivBasic` is new (Decision 5). Their `run` and `dispatch` arms call the drivers; `run_watdiv`, `run_watdiv_basic` and `run_gen_watdiv` are deleted, and with them `materialize.rs`'s `KERMIT_NO_BWRAP` reads and `dispatch`'s last `clear()`-then-old-function arm. `cli_gen_scale_range.rs` gains `watdiv-basic`; `required_flags_parse_as_required_yaml_keys` gains `watdiv` and `watdiv-basic`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/kinds/watdiv.rs`, `kermit/src/kinds/mod.rs`, `kermit/src/bench/gen.rs`, `kermit/src/materialize.rs`, `kermit/tests/cli_gen_scale_range.rs`
  - **Interfaces:** exposes the `Watdiv` and `WatdivBasic` markers, `WatdivTools`; uses Story 4's trait and drivers and Story 3's `parse_gen`
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 bash -c 'cargo test -p kermit --bin kermit && cargo test -p kermit --test cli_watdiv_gen --test cli_gen_scale_range --test watdiv_correctness'`
  - **Review:** deep (a changed environment-variable meaning, a new subcommand, the last kind)
  - **Reading:** spec "The kind layer", Decisions 5, 7, 8; names contract "Kind layer" (WatDiv); `kermit/src/materialize.rs:330-400`; `kermit/src/bench/gen.rs:138-186,365-435`; `kermit/tests/cli_watdiv_gen.rs`

- **Story 9** — The shared hook, and SNAP and H2O onto it.

  Adds `kermit/tests/common/generated.rs` with `bench_dir` and `assert_every_cell_answers_expected`, as the spec's "The shared hook" describes: it fails, naming the query, when a query lacks `expected`; runs `bench probe <bench> -i all -a all --optimiser <o> --report-json <tmp>` once per `Optimiser::value_variants()` with `XDG_CACHE_HOME=<cache_home>`, `KERMIT_WORKSPACE` at a temporary directory holding an empty `benchmarks/`, and `CRITERION_HOME` in a temporary directory; requires exit 0, `verified: true` on every report row, and every query under every `(data_structure, algorithm)` pair present; and skips with a stderr line off Linux. `snap_generate.rs` and `h2o_generate.rs` generate into `bench_dir(…)` and call the hook once per fixture; their `cell_mismatches` and hand-listed cells are deleted, and their kind-specific checks stay.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/tests/common/generated.rs`, `kermit/tests/common/mod.rs`, `kermit/tests/snap_generate.rs`, `kermit/tests/h2o_generate.rs`
  - **Interfaces:** exposes `bench_dir`, `assert_every_cell_answers_expected`; uses `common::cli::{kermit_bin, reports_of}`
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 cargo test -p kermit --test snap_generate --test h2o_generate`
  - **Review:** deep (a correctness check that must not pass vacuously)
  - **Reading:** spec "The shared hook", "Existing tests moved onto it", Decision 12; `kermit/tests/common/cli.rs`; `kermit/tests/snap_generate.rs`; `kermit/tests/h2o_generate.rs`; `kermit/tests/common/mod.rs`

- **Story 10** — JOB and LUBM onto the hook.

  `job_generate.rs` generates each template into `bench_dir(…)` and calls the hook (its private `mismatches` and hand-listed cells go; the DuckDB skip, the hand-derived relations and the recount stay). `lubm_mini_oracle.rs` passes its hand-counted answers as each `LubmQuerySpec`'s `expected_cardinality`, generates into `bench_dir(…)`, and calls the hook, so its 14 queries run on all four cells; its seek-strategy loop and its derivation table stay. If a hash cell disagrees because a repeated triple survives partitioning, the developer reports it as a finding (BLOCKED, with the counts) instead of changing a count or dropping a cell.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/tests/job_generate.rs`, `kermit/tests/lubm_mini_oracle.rs`
  - **Interfaces:** uses Story 9's `bench_dir` and `assert_every_cell_answers_expected`
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 cargo test -p kermit --test job_generate --test lubm_mini_oracle`
  - **Review:** deep (LUBM's first run on the hash cells may surface a real finding)
  - **Reading:** spec "Existing tests moved onto it"; `kermit/tests/job_generate.rs:380-600`; `kermit/tests/lubm_mini_oracle.rs`

- **Story 11** — The cross-kind tests and Decision 9's CLI test.

  In `kermit/src/kinds/mod.rs`'s tests: `kinds_agree_across_yaml_cli_and_dispatch` (the set of `kind` values across the committed `benchmarks/*.yml` equals the set of `bench gen` subcommands read from `Cli::command()`, and `kind_name` of each committed spec equals its YAML `kind`) and `tool_flags_name_their_environment_variables` (every `Tools` argument's `Arg::get_env` against the table `KERMIT_WATDIV_BIN`, `KERMIT_NO_BWRAP`, `KERMIT_LUBM_JAR`, setting no environment variable). New `kermit/tests/cli_gen_kinds.rs`: a cache under `XDG_CACHE_HOME` whose `meta.json` `spec_hash` differs from its workspace YAML's (`KERMIT_WORKSPACE`), then `bench run --force` with `KERMIT_LUBM_JAR=/nonexistent` (and, in the second test, `KERMIT_WATDIV_BIN=/nonexistent`) exits 1 and leaves the cache directory byte-identical. Linux only, as the other tests that set `XDG_CACHE_HOME`.

  - **Developer:** intermediate-dev
  - **Files:** `kermit/src/kinds/mod.rs`, `kermit/tests/cli_gen_kinds.rs`
  - **Interfaces:** uses `kind_name`, `Cli`, every kind's `Tools`
  - **Verify:** `direnv exec . env CARGO_INCREMENTAL=0 bash -c 'cargo test -p kermit --bin kermit kinds && cargo test -p kermit --test cli_gen_kinds'`
  - **Review:** deep
  - **Reading:** spec "Tying the kinds together", "Spec and kind tests", "CLI tests", Decision 9; `kermit/src/materialize.rs:180-240`; `kermit/tests/cli_snap_gen.rs:1-60`

- **Story 12** — The recipe and the documentation.

  Writes CLAUDE.md's "Adding a static benchmark" (today's text) and "Adding a generated benchmark" (the spec's recipe, steps 0–9) in "Extending the System", plus the Build Commands lines for `bench gen watdiv-basic` and `bench gen lubm --queries`, the kind layer in the `kermit` architecture entry, and the declarative-generator gotcha naming `kermit/src/kinds/`. Then every edit of the spec's "Documentation" section: `benchmarks/README.md`, `docs/benchmarks/README.md`, `docs/benchmarks/WATDIV.md`, `docs/benchmarks/LUBM.md`, `docs/specs/benchmarking-architecture.md`. Docs only.

  - **Developer:** intermediate-dev
  - **Files:** `CLAUDE.md`, `benchmarks/README.md`, `docs/benchmarks/README.md`, `docs/benchmarks/WATDIV.md`, `docs/benchmarks/LUBM.md`, `docs/specs/benchmarking-architecture.md`
  - **Interfaces:** documents every name of Stories 1–11
  - **Verify:** `bash -c 'grep -q "^### Adding a generated benchmark" CLAUDE.md && grep -q "^### Adding a static benchmark" CLAUDE.md && ! grep -n "generator-driven RDF" docs/benchmarks/README.md'`
  - **Review:** light (docs only; checked against the spec's recipe)
  - **Reading:** spec "The recipe (CLAUDE.md, \"Extending the System\")", "Documentation"; `CLAUDE.md` (sections "Extending the System", "Build Commands", "Gotchas"); `benchmarks/README.md:55-115,250-262`; `docs/benchmarks/README.md`

<!-- plan_join: the stories below are generated from the drafts; edit the drafts, then re-join -->
