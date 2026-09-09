# Runner Consolidation

**Date:** 2026-09-09
**Status:** Approved design, not yet implemented
**Scope:** Sub-project B of the benchmarking-flow improvement series
(A: sweep hardening — landed 2026-09-09; B: runner consolidation;
C: answer verification; D: Rust–Python contract; E: download integrity).

## Motivation

Two findings from the 2026-09-09 architectural analysis:

1. `bench join` is a hand-rolled third sibling. It type-erases the execution
   family into a `JoinRunner` closure (`load_query_runner` in
   `kermit/src/main.rs`), so it measures only the query time, reports no
   `ds_layout_*` / `ds_config_*` axes, has no `--metrics`, `--queries-per-build`
   or `--ds-config`, and assembles its metadata and axes by hand. It is the
   one path where the report cannot carry the full optimisation-axis set.
2. `kermit/src/main.rs` is ~2 000 lines holding clap definitions, all three
   Criterion runners, every handler, and the generator commands. The runners
   are private to the binary, so nothing below the black-box CLI tests can
   exercise them.

The fix is one generic runner for every measured join, fed by a small
`Workload` value that both `bench run` and `bench join` construct, living in
a `bench` module beside `execution.rs`.

## Decisions taken during brainstorming

- **`bench join` adopts `bench run`'s Criterion naming.** Group
  `{prefix}/{workload}/{query}/{ds}/{algo}`, functions `insertion`,
  `iteration`, `end_to_end`, `space/{relation}`. Old ad-hoc `bench join`
  directories (`{name}/{ds}/{algo}`) are no longer matched by Criterion's
  change detection; the JSON reports for old runs still resolve. `kermit-lab`
  treats the group name as opaque, so it needs no change.
- **Ad-hoc identity.** `bench join` uses workload `adhoc` and the query
  file's stem as the query name, with default prefix `join`. Example:
  `join/adhoc/triangle/TreeTrie/LeapfrogTriejoin`. Report axes gain
  `benchmark: "adhoc"` and `query: "<stem>"`.

## 1. `Workload`: what the runner consumes

```rust
// kermit/src/bench/workload.rs
pub struct NamedQuery {
    pub name: String,
    pub query: JoinQuery,
}

pub struct Workload {
    /// Benchmark name, or `"adhoc"` for `bench join`.
    pub name: String,
    /// Relation files, already cached / resolved; loaded by the family.
    pub relation_paths: Vec<PathBuf>,
    pub queries: Vec<NamedQuery>,
}

impl Workload {
    /// `bench run`: ensure the definition's relations are cached (via
    /// `kermit_bench::cache::ensure_cached`), parse every query string,
    /// and apply the optional `-q` filter. An unknown filter name is an
    /// error listing the available names (today's message, unchanged).
    pub fn from_definition(
        def: &BenchmarkDefinition, workspace_root: &Path, query_filter: Option<&str>,
    ) -> anyhow::Result<Self>;

    /// `bench join`: the given relation paths verbatim, one query parsed
    /// from `query_path`, named by the file stem. A path without a stem is
    /// an error.
    pub fn adhoc(relation_paths: Vec<PathBuf>, query_path: &Path) -> anyhow::Result<Self>;
}
```

The alternative — synthesising a `BenchmarkDefinition` for `bench join` — was
rejected: `validate()` requires workspace-relative relation paths whose stem
equals the relation name, and the query arrives as a `.dl` file, not a YAML
string. `Workload` is the value the runner actually needs.

## 2. Module layout

```
kermit/src/bench/
├── mod.rs        Metric (moved from main.rs), re-exports, the two
│                 Criterion-builder helpers, add_space_bench
├── workload.rs   Workload, NamedQuery, constructors + unit tests
├── run.rs        run_benchmark<F: ExecutionFamily>(family, &Workload, prefix, …),
│                 dispatch_run_bench, resolve_sweep
└── ds.rs         run_ds_bench<F: RelationFamily>, dispatch_ds_bench
```

`run_benchmark`'s body moves unchanged except for its inputs: it takes
`&Workload` and a `prefix: &str` (resolved by the handler from `--name` or the
command's default) instead of `&BenchmarkDefinition`, a query filter, and the
hard-coded `DEFAULT_RUN_GROUP`. It no longer calls `ensure_cached` or parses
queries; `Workload::from_definition` did that.

`main.rs` keeps: clap types (`Cli`, `QueryArgs`, `BenchArgs`, the selectors,
`BenchSubcommand`, `GenSubcommand`), the thin handlers (`run_join`,
`run_bench_join`, `run_ds_bench_command`, `run_bench_run_command`, list /
fetch / clean / gen), `resolve_benchmarks`, `describe_benchmark_status`,
`load_query_runner` + `JoinRunner` (still the right tool for the one-shot
`kermit join`), and `main`. The `DEFAULT_*_GROUP` constants stay with the
handlers that apply them.

## 3. The new `bench join`

Clap surface:

```
kermit bench [BenchArgs] join
    --relations <PATH>... --query <PATH> -a <ALGO> -i <DS> [--optimiser …]
    [--ds-layout-hasher …] [--ds-layout-pruning …]      (as today, via QueryArgs)
    [--ds-config KEY=VALUE,…]                            (new: ConfigChoices)
    [-m insertion iteration space end-to-end]            (new; default = the three)
    [--queries-per-build K]                              (new)
    [-o <PATH>]                                          (kept)
```

Handler flow: validate layout/config for the concrete structure (reusing
`validate_layout_choices` / `validate_config_choices` by lifting the
`IndexStructure` into its selector), resolve the `Execution` cell with
`Execution::for_pair` (incompatible pair stays a usage error), if `-o` is set
run the query once through the family and write CSV (unchanged behaviour),
build `Workload::adhoc`, open a `ReportSink` of kind `Join`, dispatch to
`run_benchmark` with prefix `--name` or `join`, push, finish.

`kermit join` (top-level) is unchanged and keeps deliberately omitting
`--ds-config`.

Report for `bench join` after the change: `kind: "join"`, axes
`benchmark`, `query`, `data_structure`, `algorithm`, `optimiser`, `tuples`,
`queries_per_build` (only with `end-to-end`), plus `ds_*` axes; metadata
block header becomes the runner's `bench run metadata`. `relations` (the
old count axis) is dropped — it duplicated what the metadata lists.

## 4. Tests

- **Unit** (`bench/workload.rs`): `from_definition` with no filter keeps
  every query in order; with a matching filter keeps exactly one; with an
  unknown name errors and the message lists the available names; `adhoc`
  names the query by the file stem and rejects a path with no stem. These
  use `path:`-style local relations in a temp workspace so `ensure_cached`
  needs no network.
- **CLI, updated** (`kermit/tests/cli_join_tests.rs`): `cli_bench_runs_criterion`
  and `cli_bench_default_name` are re-pointed at the new naming
  (`test-intersect/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin`,
  `join/adhoc/…`) and the `bench run metadata` header. Listed here so the
  change is deliberate, not drift.
- **CLI, new**: `bench join -i hash-trie -a hash-triejoin --ds-layout-hasher fxhash
  --ds-config load-factor=0.5` reports `ds_layout_hasher`,
  `ds_layout_pruning`, `ds_config_load_factor`, `benchmark: "adhoc"`,
  `query: "intersect_query"`, and four `criterion_groups` entries whose
  functions are `insertion`, `iteration`, `space/first`, `space/second` (the
  two fixture relations) — mirroring the
  hash-trie `bench ds` tests in `kermit/tests/common/cli.rs`.
- **Unchanged must stay green**: every `bench run` and `bench ds` CLI suite
  (`cli_bench_run_sweep`, `cli_end_to_end_metric`, `cli_hash_trie_*`,
  `cli_optimiser_choice`), which proves the move is behaviour-preserving for
  those two commands.

## Out of scope

- The `bench ds` function-name asymmetry (`{ds}/iteration` vs `iteration`).
- A verify step (sub-project C); a `-i all` sweep on `bench join`.
- Any change to `execution.rs`, `options.rs`, `db.rs`, `bench_report.rs`,
  or the report `schema_version` (no field is renamed or retyped; `bench join`
  gains keys it lacked, which is additive).

## Verification

Full gate: `cargo test`, `cargo clippy --all-targets` with `-Dwarnings`,
`cargo doc` with `-Dwarnings`, nightly `cargo fmt --all --check` inside
`nix develop`. Plus a manual `bench join` on the triangle fixtures with
`-i hash-trie --ds-config load-factor=0.5 -m space`, checking the report's
axes by eye, and one `bench run triangle -i all -a all` to confirm the
Criterion directory names are byte-identical to a pre-change run.
