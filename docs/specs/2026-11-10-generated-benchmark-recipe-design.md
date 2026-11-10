# A Recipe for Generated Benchmarks: One Spec Type and One Kind per Generator

**Date:** 2026-11-10
**Status:** Design approved in conversation: decision by decision, then section by section
(2026-11-10).
**Scope:** How a generated benchmark (`generator:` in a `benchmarks/*.yml`) is declared, wired and
tested, for all six kinds (`watdiv`, `watdiv-basic`, `lubm`, `snap`, `h2o`, `job`). Each kind gets
one spec struct in `kermit-bench` that is both its YAML block and its `bench gen` flag set, and one
`BenchmarkKind` impl in the binary that `bench gen` and `materialize` share. A shared test hook runs
every generated benchmark's queries on every cell under every optimiser. A recipe in CLAUDE.md's
"Extending the System" makes the steps the contract. No timed code, no spec hash, no `meta.json`
schema and no report schema changes.
**Related:**
- `kermit-generate/src/lib.rs` (the cache contract, #95), `kermit-rdf/src/generator.rs`
  (`META_SCHEMA_VERSION`, `OUTDATED_BEFORE`).
- `docs/specs/2026-10-15-h2o-benchmark-design.md`, `docs/specs/2026-10-15-snap-benchmark-design.md`,
  `docs/specs/2026-10-15-job-benchmark-design.md`: the three generators whose wiring this design
  factors out.
- `docs/benchmarks/README.md`: the benchmark doc layout the recipe's doc step points at.

## Summary

Adding SNAP, H2O and JOB each meant editing about a dozen files across four crates, and nothing
wrote the steps down: CLAUDE.md's "Adding a new benchmark" is two sentences about YAML,
`benchmarks/README.md` has a static-only recipe and `docs/benchmarks/README.md` covers the docs
alone. Each kind's spec-to-generator logic is written twice, once in `kermit/src/materialize.rs`
(the YAML path) and once in `kermit/src/bench/gen.rs` (`bench gen`), and the two copies have
drifted (five behaviour differences, listed under Decisions). The generator end-to-end tests list
their cells by hand, and SNAP's, H2O's and JOB's run three of the four, so no generated benchmark is
checked on `HashIndex` / `BinaryHashJoin`.

This design gives each kind one spec type and one kind implementation, so both paths run the same
code, and moves every generated benchmark's correctness check onto one hook that covers every cell
and optimiser the binary knows, including ones added later.

## Decisions

1. **Restructure, not only document** (option C of three: recipe only; recipe plus a shared test
   hook; recipe, hook and less wiring per kind).
2. **All six kinds**, the `kermit-rdf` ones included, not only the `kermit-generate` ones.
3. **Typed kinds with exhaustive matches** (approach 1 of three). Rejected: a `generator_kinds!`
   macro expanding the binary's sites (saves one line per kind, cannot reach `GeneratorSpec` in
   `kermit-bench`, and puts clap derives and `--help` text inside macro output); a runtime
   registry over `{ kind: String, params: Mapping }` (loses load-time validation and exhaustive
   matches, and hashing needs the typed spec anyway, see "Spec hashes").
4. **The spec is the flag set.** Each spec struct derives `clap::Args` behind an optional `clap`
   feature of `kermit-bench`, which only the binary enables. The binary's mirror enums go.
5. **`bench gen watdiv-basic` is added**, named `watdiv-basic-{scale}-{tag}`: every kind has a
   subcommand.
6. **`bench gen lubm` gains `--queries`**, which comes with `LubmSpec`.
7. **`KERMIT_NO_BWRAP` becomes a boolean on both paths**, through clap's `FalseyValueParser`:
   unset, `0`, `false`, `no`, `off` or empty keep bwrap; anything else disables it, as
   `--no-bwrap` does. The one change: `KERMIT_NO_BWRAP=0` used to disable bwrap on the YAML path
   (`materialize.rs` tested `var_os(…).is_none()`). Nothing in the repository sets it.
8. **Every `bench gen` validates its spec and its name before it runs.** `bench gen watdiv` did
   not, and gains the non-empty-directory note the others print.
9. **`materialize` runs `preflight` before `--force` removes a drifted or outdated cache.** Today
   it removes the directory and then discovers a missing tool, leaving no cache.
10. **Tags stay as they are**: `bench gen` records `--tag` in `meta.json`, the YAML path records
    the benchmark name.
11. **One summary line**: every `bench gen` prints `[gen <kind>] wrote <dir> (<summary>)`. SNAP's
    and H2O's `<kind> benchmark written to <dir>` goes.
12. **The shared hook runs through the binary** (`bench probe -i all -a all`), not in-process,
    because `Execution` is a binary-only module: an in-process test can only list cells by hand.

## Background: the wiring today

A generated kind is reached from five places:

| Site | What it holds per kind |
|---|---|
| `kermit-bench/src/definition.rs` | a `GeneratorSpec` struct variant (fields inline), an arm of `validate_generator` (line 596) |
| `kermit/src/materialize.rs` | an arm of `dispatch` (line 277) and a `run_<kind>` that builds the generator with `spec_hash: Some` |
| `kermit/src/bench/gen.rs` | a `GenSubcommand` variant, an arm of `run`, and a `run_gen_<kind>` that builds the same spec from flags and the same generator with `spec_hash: None`; for SNAP and H2O, clap mirrors of the spec's value enums (`SnapGraphArg`, `SnapQueryArg`, `H2oOrderArg`) and tests pinning their names |
| `kermit-generate/src/<kind>` or `kermit-rdf` | the generator |
| `kermit/tests/<kind>_generate.rs` | a private `cell_mismatches` (or `mismatches`) and a hand-listed set of cells |

The two paths differ today:

- `watdiv-basic` has no `bench gen` subcommand.
- `bench gen lubm` always writes the 14 queries; `kind: lubm` takes `queries:`.
- `bench gen` reads `--watdiv-bin` / `--lubm-jar` with clap `env` fallbacks; `materialize` reads
  `KERMIT_WATDIV_BIN` / `KERMIT_LUBM_JAR` itself, and `KERMIT_NO_BWRAP` by presence.
- `bench gen watdiv` validates nothing before it runs.
- `bench gen` records `--tag`, the YAML path the benchmark name.

## Design

### Spec types (`kermit-bench/src/definition.rs`)

`GeneratorSpec` keeps its six variants, its tag and its renaming; each variant wraps a named struct
instead of holding its fields:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum GeneratorSpec {
    Watdiv(WatdivSpec),
    WatdivBasic(WatdivBasicSpec),
    Lubm(LubmSpec),
    Snap(SnapSpec),
    H2o(H2oSpec),
    Job(JobSpec),
}
```

The structs are `WatdivSpec { scale, stress: WatdivStressSpec }`, `WatdivBasicSpec { scale }`,
`LubmSpec { scale, seed, threads, start_index, ontology, queries }`,
`SnapSpec { graph, url, sha256, queries }`, `H2oSpec { scale, seed, order }` and
`JobSpec { template }`, in today's field order, each field with today's serde attributes
(`default`, `default = "…"`, `skip_serializing_if`) moved verbatim. `WatdivStressSpec` is unchanged
and flattened into `WatdivSpec`'s flags. Each struct has `impl From<…> for GeneratorSpec`.

Each struct has `pub fn validate(&self) -> Result<(), String>`, the body of today's
`validate_generator` arm for its kind; `validate_generator` becomes one line per kind, mapping the
reason into `BenchError::Invalid { name, reason }` as today.

### The `clap` feature

`kermit-bench/Cargo.toml` gains `clap = { version = "4.4", optional = true, features = ["derive",
"env"] }` (the binary's requirement) and a feature `clap`. `kermit/Cargo.toml` enables it; `kermit-rdf` and `kermit-generate` do not. With the
feature:

- each spec struct derives `clap::Args` (`#[cfg_attr(feature = "clap", derive(clap::Args))]`), with
  `#[cfg_attr(feature = "clap", arg(…))]` on each field: `long`, `value_name`, `value_enum`,
  `value_delimiter = ','` for lists, `default_value_t` equal to the serde default, the H2O floor
  and the `>= 1` scales as `value_parser` ranges, and SNAP's `graph` / `url` / `sha256`
  conflicts and requirements as today's `GenSubcommand::Snap` declares them;
- `SnapGraph`, `SnapQuery` and `H2oOrder` derive `clap::ValueEnum`, whose names are the serde names
  (`com-amazon`, `triangle`, `shuffled`);
- where a field's rustdoc does not read as `--help` text, it carries an explicit `arg(help = …)`.

Flag names come from the field names and equal today's (`--scale`, `--seed`, `--start-index`,
`--threads`, `--ontology`, `--max-query-size`, `--query-count`, `--constants-per-query`,
`--allow-join-vertex`, `--graph`, `--url`, `--sha256`, `--queries`, `--order`, `--template`).
`SnapGraphArg`, `SnapQueryArg`, `H2oOrderArg`, their `From` impls and their name-pinning tests are
deleted. `kermit-bench` must still compile, lint and document without the feature.

### Spec hashes

`GeneratorSpec::spec_hash` (`definition.rs:322`) is the SHA-256 of `serde_yaml::to_string(spec)`;
`materialize` compares it with each cache's `meta.json`. A change to how any committed spec
serializes makes every cache of that kind `SpecDrift`, refused without `--force`, the pilot sweep's
JOB caches included. Serde serializes an internally tagged newtype variant wrapping a struct
exactly as a struct variant with the same fields: the tag, then the fields in declaration order.
The design relies on that and pins it rather than assuming it:

- `kermit-bench/tests/spec_hashes.rs` (new, the first commit, green on today's code) records the
  `spec_hash` of every committed `benchmarks/*.yml` with a `generator:` block (42 files on
  2026-11-10, all six kinds), read from the workspace, plus, per kind, the hash of a spec that
  omits every optional key and of the same spec with every default written out, which must be
  equal. The test fails naming the file and both hashes.
- Typing is also why the hash cannot be taken over the raw YAML mapping: today an omitted `seed:`
  hashes as `seed: 108` and key order does not matter, because the YAML is parsed into the typed
  spec and the typed value is serialized.

### The kind layer (`kermit/src/kinds/`, new)

```rust
/// One generated-benchmark kind: how `bench gen <kind>` names it, and how both
/// `bench gen` and `materialize` turn its spec into a cache directory.
pub(crate) trait BenchmarkKind {
    /// The YAML `kind` and the `bench gen` subcommand (`"h2o"`).
    const KIND: &'static str;
    /// The kind's spec (`kermit_bench::H2oSpec`).
    type Spec: Into<GeneratorSpec> + Clone;
    /// External-tool overrides, each a flag with an environment fallback
    /// (`--watdiv-bin` / `KERMIT_WATDIV_BIN`); `NoTools` for a kind with none.
    type Tools: clap::Args + clap::FromArgMatches;
    /// What `preflight` found: a resolved binary or jar, a located DuckDB.
    type Ready;
    /// `bench gen`'s benchmark name (`h2o-{scale}-{tag}`).
    fn bench_name(spec: &Self::Spec, tag: &str) -> anyhow::Result<String>;
    /// Checks the host can run the generator. Creates and deletes no file.
    fn preflight(spec: &Self::Spec, tools: Self::Tools) -> anyhow::Result<Self::Ready>;
    /// Writes the benchmark into `target.out_dir`; returns `bench gen`'s summary.
    fn generate(spec: &Self::Spec, ready: &Self::Ready, target: Target<'_>) -> anyhow::Result<String>;
}
```

`kinds/mod.rs` holds the trait, `NoTools` (an empty `#[derive(clap::Args)]` struct) and
`tools_from_env::<T>()`, which parses `T` from an empty argument list through
`T::augment_args(Command::new(…))`, so every field takes its `env` fallback or its default: one
clap definition gives both paths the same semantics.

One file per kind holds that kind's logic once, merged from today's `run_<kind>` and
`run_gen_<kind>`:

| File | Kinds | `Tools` | `Ready` from `preflight` | `bench_name` |
|---|---|---|---|---|
| `watdiv.rs` | `Watdiv`, `WatdivBasic` | `WatdivTools { watdiv_bin: Option<PathBuf>` (`--watdiv-bin`, `KERMIT_WATDIV_BIN`), `no_bwrap: bool` (`--no-bwrap`, `KERMIT_NO_BWRAP`, `FalseyValueParser`) `}` | the binary (checked to exist), the vendor root, `use_bwrap` | `watdiv-stress-{scale}-{tag}`, `watdiv-basic-{scale}-{tag}` |
| `lubm.rs` | `Lubm` | `LubmTools { lubm_jar: Option<PathBuf>` (`--lubm-jar`, `KERMIT_LUBM_JAR`) `}` | the jar (checked to exist), the selected `LubmQuerySpec`s | `lubm-{scale}-{tag}` |
| `snap.rs` | `Snap` | `NoTools` | the `SnapSource` | `snap-{graph or URL stem}-{tag}` |
| `h2o.rs` | `H2o` | `NoTools` | the `KeyPlan` | `h2o-{scale}-{tag}` |
| `job.rs` | `Job` | `NoTools` (DuckDB stays `Duckdb::locate`: `KERMIT_DUCKDB_BIN`, else `PATH`) | the located `Duckdb` | `job-{template}-{tag}` |

JOB's `raw::ensure`, which builds or checks the shared IMDb cache, writes files, so it runs in
`generate`, not `preflight`, as `bench gen job` runs it after `GenTarget::prepare` today. The
`kermit-rdf` pipelines and `kermit_generate::run` are called from `generate` unchanged; neither
library's `Generator` trait changes. Each summary keeps today's fields (WatDiv: triples, relations,
queries; LUBM: pre, post, derived, relations, queries; the others: relations, queries).

### `bench gen` (`kermit/src/bench/gen.rs`)

Each `GenSubcommand` variant is written out with three flattened fields:

```rust
/// Generate an H2O db-benchmark join benchmark (#95)
H2o {
    #[command(flatten)] spec: kermit_bench::H2oSpec,
    #[command(flatten)] tools: kinds::NoTools,
    #[command(flatten)] target: GenTarget,
},
```

`run` has one line per kind, calling one generic driver, `gen::<K>(spec, tools, target)`, which:

1. names the benchmark (`K::bench_name`);
2. validates `BenchmarkDefinition { name, generator: Some(spec.clone().into()), .. }`;
3. refuses a name a workspace YAML already uses;
4. runs `K::preflight`;
5. runs `GenTarget::prepare` (the undiscovered `--output-dir` and non-empty-directory notes);
6. runs `K::generate` with `Target { spec_hash: None, tag: &target.tag, .. }`;
7. prints `[gen <K::KIND>] wrote <dir> (<summary>)`.

`--tag` and `--output-dir` keep their names on every kind; `GenTarget` replaces WatDiv's and LUBM's
own copies of them.

### `materialize` (`kermit/src/materialize.rs`)

The cache classification (hit, drift, outdated, miss) is unchanged. On a miss, or a drift or
outdated cache under `--force`, `dispatch` has one line per kind calling
`regenerate::<K>(spec, …)`, which reads `K::Tools` through `tools_from_env`, runs `K::preflight`,
and only then removes the drifted or outdated directory (if there is one), creates it, and runs `K::generate` with
`Target { spec_hash: Some(hash), tag: bench_name, .. }`. A `preflight` error is reported as today's
`generator_failed` error. `vendored_watdiv_root` and `vendored_lubm_jar` stay where `bench gen`
and the tests reach them.

### Tying the kinds together

The compiler checks `validate_generator` and `dispatch`, both exhaustive matches on
`GeneratorSpec`. `GenSubcommand` is an enum definition, so a test checks it instead:
`kinds_agree_across_yaml_cli_and_dispatch` (binary unit test) requires the set of `kind` values
across the committed `benchmarks/*.yml` to equal the set of `bench gen` subcommand names
(read from `Cli::command()`) and, for each committed spec, `kind_name(&spec)` to equal the YAML's
`kind`. `kind_name` sits beside `dispatch`: an exhaustive match on `GeneratorSpec` returning each
arm's `K::KIND`, which `dispatch`'s error messages also use. Recipe step 6 (a committed YAML per kind) is what makes the first set
complete.

## Testing

### The shared hook (`kermit/tests/common/generated.rs`, new)

```rust
/// Runs `bench probe <bench> -i all -a all --optimiser <o>` for every optimiser over a
/// generated cache under `cache_home`, and asserts every (query, cell) row was verified
/// against its `expected`.
pub fn assert_every_cell_answers_expected(cache_home: &Path, bench: &str)
```

- It first reads the cache's `benchmark.yml` and fails, naming the query, if any query lacks
  `expected`: `bench probe` skips verification for such a query, so the hook could otherwise pass
  without checking anything.
- It runs the binary once per `Optimiser::value_variants()`, with `XDG_CACHE_HOME` at
  `cache_home`, `KERMIT_WORKSPACE` at an empty temporary workspace, `CRITERION_HOME` and
  `--report-json` in temporary paths, and requires exit 0.
- It reads the report and requires `verified: true` on every row and every query under every
  `(data_structure, algorithm)` pair the report holds.
- `-i all -a all` expands to every valid cell, so a cell added to `Execution`, like an optimiser
  added to `Optimiser`, reaches every generator's test with no edit. It also covers `bench run`'s
  own loading path (`Workload::prepare`, `RelationFamily::build_index`).
- It runs default Layouts only, and no engine; Layouts are the join suite's job, engines
  `engine_oracle.rs`'s.
- Like the other tests that set `XDG_CACHE_HOME`, it runs on Linux only (`dirs` honours the
  variable only there) and skips with a stderr line elsewhere. CI runs on Linux.

Each kind's test generates its fixture in-process into `<cache_home>/kermit/benchmarks/<name>/`, as
today, and calls the hook once per fixture.

### Existing tests moved onto it

- `snap_generate.rs`, `h2o_generate.rs`, `job_generate.rs`: the private `cell_mismatches` /
  `mismatches` and their hand-listed cells are deleted; one hook call per fixture replaces them.
  Each kind's own checks stay (relation contents, `meta.json` extras, golden digests, DuckDB's
  recount). This closes the `HashIndex` / `BinaryHashJoin` gap.
- `lubm_mini_oracle.rs`: it passes its hand-counted answers as each `LubmQuerySpec`'s
  `expected_cardinality`, so the emitted `benchmark.yml` carries them, and calls the hook, so the 14
  queries run on all four cells. Its seek-strategy loop over the sorted tries stays, since the hook
  runs default Layouts only; its derivation table stays the source of the numbers. If a hash cell
  disagrees because a repeated triple survives partitioning (`HashTrie` and `HashIndex` keep
  duplicates), that is a finding to report, not a test to weaken.
- WatDiv has no answer counts (the vendored binary emits none), so the hook does not apply;
  `e2e_watdiv.rs` and `e2e_watdiv_basic.rs` stay as they are.

### Spec and kind tests

- `kermit-bench/tests/spec_hashes.rs` (see "Spec hashes").
- Per kind, in the binary: parsing `bench gen <kind>` with only its required flags gives the spec
  that parsing a YAML block with only its required keys gives, so clap's defaults and serde's
  cannot drift apart.
- `kinds_agree_across_yaml_cli_and_dispatch` (see "Tying the kinds together").
- `tool_flags_name_their_environment_variables`: every `Tools` argument names its variable, against
  the table `KERMIT_WATDIV_BIN`, `KERMIT_NO_BWRAP`, `KERMIT_LUBM_JAR`, read with `Arg::get_env`
  from the `Command`, never by setting variables in-process (racy across parallel tests).
- Unit tests in each `kinds/<kind>.rs`: its `bench_name` (the SNAP naming tests move here from
  `gen.rs`), and `preflight` refusing a missing tool with an error that names both the flag and the
  variable.

### CLI tests

- `kermit/tests/cli_gen_kinds.rs` (new), decision 9: a drifted LUBM cache with
  `KERMIT_LUBM_JAR=/nonexistent` makes `bench run --force` exit 1 and leaves the cache directory
  byte-identical; the same for WatDiv with `KERMIT_WATDIV_BIN=/nonexistent`. Neither needs Java or
  the WatDiv binary, since both fail in `preflight`.
- `cli_gen_scale_range.rs` gains `watdiv-basic` (decision 5).
- `cli_snap_gen.rs`, `cli_job_gen.rs`, `cli_watdiv_gen.rs` pass unchanged, flag spellings being
  unchanged, except `cli_snap_gen.rs:251`'s `snap benchmark written to`, which becomes the
  `[gen snap] wrote` line (decision 11).

## The recipe (CLAUDE.md, "Extending the System")

"Adding a new benchmark" becomes two subsections. "Adding a static benchmark" is today's YAML text.
"Adding a generated benchmark" is:

0. **Classify.** Relations downloaded or committed: a static benchmark, a YAML file alone. A
   generator that writes its relations itself: `kermit-generate`. A generator that runs an external
   RDF tool and partitions its N-Triples: `kermit-rdf`, which never gains a `kermit-ds` dependency.
1. **Spec.** `<Kind>Spec` in `kermit-bench/src/definition.rs`: a `GeneratorSpec` variant and a
   `From`, serde defaults, `validate`, clap attributes behind the `clap` feature, value enums
   deriving `ValueEnum`. Add its committed YAMLs and its omitted-defaults form to
   `kermit-bench/tests/spec_hashes.rs`.
2. **Generator.** `kermit-generate/src/<kind>/` implementing `kermit_generate::Generator`, with
   `pub mod <kind>;` in `lib.rs` and its own dependencies in `kermit-generate/Cargo.toml`; or a
   `kermit_rdf::generator::Generator` over `process_artifacts`.
3. **Answer counts.** Every query's `expected` comes from code that shares nothing with the joins:
   a counting module (SNAP's `counts.rs`), a bitmap (H2O), an independent system (JOB's DuckDB
   `COUNT(*)`) or a published reference (LUBM(1, 0)). A kind without one says why in its doc
   (WatDiv).
4. **Reproducibility.** The output is a pure function of the spec: no dependence on the clock,
   thread order or hash-map order; random numbers from an in-repo generator (`rand`'s values change
   between versions); downloads through `kermit_bench::cache::download_verified` with a pinned
   SHA-256; external queries with a pinned row order (`ORDER BY`); golden digests of a small plan as
   the pin.
5. **Kind.** `kermit/src/kinds/<kind>.rs`: a marker type and its `BenchmarkKind` impl, then one
   line each in `GenSubcommand`, `bench::gen::run`, `materialize::dispatch` and
   `validate_generator`. The compiler and `kinds_agree_across_yaml_cli_and_dispatch` name a missed
   site.
6. **YAML.** At least one committed `benchmarks/<name>.yml` with a `generator:` block.
7. **Tests.** `kermit-generate/tests/<kind>.rs` (what `run` leaves: files, `benchmark.yml`,
   `meta.json`; golden digests); `kermit/tests/<kind>_generate.rs` (a small fixture, then
   `assert_every_cell_answers_expected`); `kermit/tests/cli_<kind>_gen.rs` (`XDG_CACHE_HOME`,
   `KERMIT_WORKSPACE`, `CRITERION_HOME`, a loopback HTTP server for downloads). A test that needs an
   external tool skips with a stderr line when it is absent, and the doc's Tests table says so.
8. **Afterwards.** When the output changes for an unchanged spec, bump `META_SCHEMA_VERSION` and add
   the kind's floor to `OUTDATED_BEFORE`, so old caches show as `stale` instead of passing as hits.
9. **Docs.** `docs/benchmarks/<NAME>.md` in the layout `docs/benchmarks/README.md` defines; its
   index entry and a comparison-matrix column with every row filled; the kind's field table in
   `benchmarks/README.md`; in CLAUDE.md, a `bench gen <kind>` line in Build Commands, an entry in
   Benchmark Reference Docs, and the kind lists (the `kermit-generate` architecture entry, the
   declarative-generator gotcha).

Do **not** change other kinds' generators while adding one (Priorities item 6).

## Documentation

- CLAUDE.md: the recipe above; the `bench gen watdiv-basic` and `bench gen lubm --queries` lines in
  Build Commands; the kind layer in the `kermit` architecture entry; the declarative-generator
  gotcha naming `kermit/src/kinds/` instead of `materialize.rs`'s per-kind functions.
- `benchmarks/README.md`: "Adding a new benchmark" keeps its static steps and links to the recipe
  for generated ones; the `KERMIT_NO_BWRAP` semantics where bwrap is described.
- `docs/benchmarks/README.md`: "generator-driven RDF benchmarks" loses "RDF" (SNAP, H2O and JOB are
  not); the layout's `## Pipeline` row reads "`kermit-rdf` or `kermit-generate` stages"; its
  "Adding a new benchmark" steps become the recipe's step 9.
- `docs/benchmarks/WATDIV.md` and `docs/benchmarks/LUBM.md`: the `bench gen watdiv-basic` command,
  LUBM's `--queries` flag, and `KERMIT_NO_BWRAP` in their flag tables.
- `docs/specs/benchmarking-architecture.md`: "for generator-driven (watdiv/lubm) benchmarks"
  becomes every generator kind, through the kind layer.

## Out of scope

- A runtime kind registry or a macro over the kinds (Decision 3).
- Changing `kermit_generate::Generator`, `kermit_rdf::generator::Generator`, `process_artifacts`,
  `kermit_generate::run` or any generator's output.
- A `--duckdb-bin` flag for JOB: `Duckdb::locate` keeps `KERMIT_DUCKDB_BIN`.
- What `meta.json`'s `tag` records (Decision 10).
- Layouts and engines in the shared hook.
- A test that every kind has a `docs/benchmarks/` page.

## Premises (checked 2026-11-10)

- `spec_hash` serializes the typed spec with `serde_yaml` (`kermit-bench/src/definition.rs:322`);
  no `GeneratorSpec` variant uses `deny_unknown_fields`.
- 42 committed `benchmarks/*.yml` carry a `generator:` block, covering all six kinds.
- `validate_generator` is at `definition.rs:596`; `dispatch` at `materialize.rs:277`, called after
  `fs::remove_dir_all` of a drifted or outdated cache under `--force`.
- `KERMIT_NO_BWRAP` is read only at `materialize.rs:356` and `:391`, by presence.
- `bench probe` flattens `RunSelection`, which flattens `PlannerArgs` (`--optimiser`), and aborts on
  a count that differs from `expected`.
- `SnapGraphArg`, `SnapQueryArg` and `H2oOrderArg` and their name tests are in
  `kermit/src/bench/gen.rs`; `cli_snap_gen.rs:251` asserts `snap benchmark written to`.
- The generator end-to-end tests list their cells by hand: `snap_generate.rs:143`,
  `h2o_generate.rs:124`, `job_generate.rs:456`; `lubm_mini_oracle.rs:215` runs LFTJ cells only.
- `LubmQuerySpec` carries `expected_cardinality`, written into `benchmark.yml` as `expected`.
- clap fills `Args` fields from their `env` fallbacks when parsing an empty argument list (clap's
  `Arg::env` documentation). Its documented pattern for a boolean read from the environment names
  `FalseyValueParser`; the design names it explicitly, so the accepted values are clap's falsey set
  whatever `SetTrue`'s default parser accepts.
- The workspace is on edition 2021.

## Sequencing

Each step is one or a few commits, with the gates green after each:

1. `kermit-bench/tests/spec_hashes.rs`, on today's code.
2. The spec structs, `validate` per struct, the `clap` feature; today's `GenSubcommand` variants
   flatten the spec structs, and the mirror enums are deleted. The drivers come in step 3.
3. `kermit/src/kinds/` and the two drivers, moving one kind at a time: H2O (no tool, trivial
   `preflight`), SNAP, JOB, LUBM, WatDiv, WatDiv-basic.
4. The shared hook; the SNAP, H2O, JOB and LUBM end-to-end tests moved onto it.
5. `cli_gen_kinds.rs`, the tool-variable test, the clap/serde default tests,
   `kinds_agree_across_yaml_cli_and_dispatch`.
6. The recipe and the documentation.

## Acceptance

- `spec_hashes.rs` is green on today's master and at the end; every existing cache stays a hit.
- No `META_SCHEMA_VERSION` or report `schema_version` change; nothing under `kermit-ds`,
  `kermit-algos`, `kermit/src/db` or `kermit/src/bench/{measure,run,cell,workload}.rs` changes, so
  no benchmark number can move.
- Every existing `bench gen` flag keeps its spelling and default. Additions: `bench gen
  watdiv-basic`, `bench gen lubm --queries`. One meaning change: `KERMIT_NO_BWRAP=0`. One message
  change: the summary line.
- Every generated benchmark with answer counts (SNAP, H2O, JOB, LUBM mini) is verified on all four
  cells under every optimiser.
- `kermit-bench` builds, lints and documents without its `clap` feature.
- The CI gates pass: `cargo test`, `cargo clippy --all-targets` with `-Dwarnings`, `cargo fmt
  --check`, `cargo doc` with `-Dwarnings`, the `python` job.
