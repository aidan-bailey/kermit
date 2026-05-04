# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Kermit is a Rust library for relational algebra research and benchmarking, built as a platform for a Masters thesis investigating the Leapfrog Triejoin algorithm across different data structures. It is a Cargo workspace with 7 crates. All keys are `usize` (dictionary-encoded). The codebase uses entirely safe Rust with no unsafe blocks. See `ARCHITECTURE.md` for detailed algorithmic descriptions, data flow, and Datalog query processing.

## Build Commands

```bash
cargo build --verbose           # Build entire workspace
cargo test --verbose            # Run all tests
cargo test --package kermit-ds  # Run a single crate's tests
cargo test test_tree_trie       # Run a single test by name
cargo clippy --all-targets --verbose  # Lint (CI uses RUSTFLAGS=-Dwarnings)
cargo fmt --all                 # Format (CI checks with --check)
cargo doc --workspace           # Generate docs (CI uses RUSTDOCFLAGS=-Dwarnings)
cargo run -- bench run triangle -i tree-trie -a leapfrog-triejoin  # Run a named benchmark
MIRIFLAGS="-Zmiri-disable-isolation" cargo miri setup && cargo miri test  # Check for UB (flag matches CI)
```

## Toolchain

Rust **nightly** (pinned in `rust-toolchain.toml`). Required components: clippy, miri, rust-analyzer, rustfmt. The `rustfmt.toml` uses `unstable_features=true` so nightly rustfmt is required.

## Development Environment

A Nix flake provides the recommended dev shell: `nix develop`. It sets up nightly Rust (matching `rust-toolchain.toml`), `git-cliff`, `cargo-expand`, Python 3.13, and configures `MIRIFLAGS` and `RUST_BACKTRACE` to match CI. The shell also exports an `LD_LIBRARY_PATH` covering `libstdc++.so.6` / `libz.so.1` so pip-installed wheels (numpy, matplotlib) used by `scripts/kermit-plot/` load on NixOS without manual workarounds.

## CI Checks (PR gate)

All of these must pass: `cargo test`, `cargo clippy` (warnings are errors), `cargo fmt --check`, `cargo doc` (doc warnings are errors), `cargo miri test`.

## Workspace Architecture

```
kermit-iters    → Core iterator traits (LinearIterator, TrieIterator). Zero dependencies.
kermit-derive   → Proc macros (#[derive(IntoTrieIter)]) for iterator boilerplate.
kermit-parser   → Datalog query parser (winnow). Parses "Q(X,Z) :- R(X,Y), S(Y,Z)."
kermit-ds       → Data structures: TreeTrie (pointer-based), ColumnTrie (column-oriented).
                  Both implement Relation + TrieIterable traits.
kermit-algos    → Join algorithms: LeapfrogJoinIter (binary), LeapfrogTriejoinIter (multi-way).
                  Generic over any TrieIterable data structure via JoinAlgo<DS> trait.
kermit-bench    → Benchmark definitions, discovery, and caching. No internal deps.
                  YAML-based benchmark declarations (supports multiple named queries per benchmark),
                  ZivaHub download, platform cache dir (~/.cache/kermit/benchmarks/ on Linux).
kermit          → CLI binary (clap). Subcommands: join, bench (join|ds|run|list|fetch|clean).
                  Provides DB abstraction layer (db::DB trait, DatabaseEngine).
                  All Criterion execution lives here (including SpaceMeasurement).
```

**Dependency flow:** `kermit-iters` → `kermit-derive`, `kermit-parser` → `kermit-ds` → `kermit-algos` (also depends on `kermit-parser`) → `kermit` (binary). `kermit-bench` is isolated (no internal deps); `kermit` depends on it.

## Key Trait Hierarchy

- **JoinIterable** (marker) → **LinearIterable** → **LinearIterator** (`key`, `next`, `seek`, `at_end`)
- **JoinIterable** (marker) → **TrieIterable** → **TrieIterator** : LinearIterator + `open`, `up`
- **Relation**: JoinIterable + Projectable — core data abstraction (`new`, `from_tuples`, `insert`, `insert_all`, `header`)
- **JoinAlgo\<DS\>**: algorithm trait decoupled from data structures
- **HeapSize**: heap-allocated byte count for space benchmarking (`heap_size_bytes()`)

## Testing Patterns

Tests use macro-generated suites that combinatorially test all data structures against all algorithms:
- `define_multiway_join_test!()` — individual parametrized test
- `define_multiway_join_test_suite!()` — generates 11 standard join patterns (unary, triangle, chain, star, self-join, existential, empty-result, single-relation, four-way-chain, wide-fanout, dead-end)
- Uses `paste!` crate for macro hygiene

Unit tests live inline in `#[cfg(test)]` blocks. Integration tests in `tests/` directories.

## Extending the System

- **New data structure**: implement `Relation` + `TrieIterable` + `HeapSize` in `kermit-ds`, create a `TrieIterator`, add to `IndexStructure` CLI enum, and add match arms in `run_ds_bench`/`run_benchmark` in `kermit/src/main.rs`.
- **New join algorithm**: implement `JoinAlgo<DS>` in `kermit-algos`, add to `JoinAlgorithm` CLI enum.
- **New benchmark**: add a YAML file in `benchmarks/` with name, description, relation URLs, and Datalog query. See `benchmarks/triangle.yml` for the schema.

## Code Style

- `rustfmt.toml` is extensively configured: `max_width=100`, `trailing_comma="Vertical"`, `imports_granularity="One"`, `group_imports="StdExternalCrate"`, `match_arm_leading_pipes="Always"`.
- Always run `cargo fmt --all` before committing — nightly rustfmt required due to unstable features.

## Gotchas

- **Miri isolation**: CI runs miri with `MIRIFLAGS="-Zmiri-disable-isolation"` and excludes `kermit` and `kermit-bench` from miri tests (Criterion and network code). Use the same flag locally or tests may fail differently. Miri also can't model `fchmod`, so tests using `std::fs::set_permissions` or `std::fs::copy` need `#[cfg_attr(miri, ignore = "...")]` (the kermit-rdf driver fs tests are gated this way).
- **git-cliff**: `cliff.toml` configures changelog generation via [git-cliff](https://git-cliff.org/). The release workflow auto-generates changelogs from conventional commits.
- **rustfmt noise on stable**: `cargo fmt --check` (or `cargo fmt -- --check`) on stable rustfmt prints ~50 lines of "unstable features" warnings because `rustfmt.toml` uses nightly settings. Filter with `2>&1 | grep -v "^Warning:"` to see actual diffs. Use `cargo +nightly fmt --all` for canonical formatting.
- **Space benchmarks**: `kermit/src/measurement.rs` contains `SpaceMeasurement` (custom Criterion `Measurement`) and `BytesFormatter`. Both `bench ds --metrics space` and `bench run --metrics space` route through `Criterion<SpaceMeasurement>` via `iter_custom`, producing `target/criterion/` output alongside the time metrics. The closure reconstructs the relation per iter and sums `heap_size_bytes()` so Criterion's calibration phase observes real work; the result is deterministic (zero-variance), so per-iter mean equals one `heap_size_bytes()` exactly.
- **No Criterion auto-plots**: `kermit/Cargo.toml` opts out of Criterion's default features (`default-features = false, features = ["rayon", "cargo_bench_support"]`) so the `plotters` dep is excluded entirely. Result: no SVG/HTML rendering, and the zero-variance panic that `SpaceMeasurement` used to trigger no longer applies. Measurement JSON (`estimates.json`, `sample.json`, `benchmark.json`, `tukey.json`) is still written per-function under `target/criterion/{group}/{directory_name}/{base,new}/`. Plot generation lives in `scripts/kermit-plot/` (Python, matplotlib + seaborn) — see `docs/specs/2026-05-04-remove-criterion-graphs-design.md`.
- **JSON bench reports**: every `kermit bench` invocation writes a machine-readable report. Default path is `bench-runs/{kind}-{unix-millis}.json` (`bench-runs/` is auto-created and gitignored at the workspace root); `--report-json <PATH>` overrides. Output is always a JSON array of `BenchReport` objects (one per query for `bench run`, exactly one for `bench join` / `bench ds`). Each object carries `metadata` (label/value pairs mirroring stderr), `axes` (a `BTreeMap<String, serde_json::Value>` of structured axis values for tooling — conventional keys: `data_structure`, `algorithm`, `query`, `benchmark`, `relation_path`, `relation_bytes`, `tuples`, `arity`, `relations`), and `criterion_groups` pointers resolving to `target/criterion/{group}/{directory_name}/`. The on-disk `directory_name` replaces `/` in `function_id` with `_` — read it from each subdir's `benchmark.json:directory_name` rather than computing it. Schema is versioned via `schema_version` (currently `2`) and lives in `kermit/src/bench_report.rs`; full key catalogue in `docs/specs/bench-report-schema.md`. Bump the version on any breaking field-name or value-type change.
- **bench `--name` semantics**: For `bench join` and `bench ds`, `--name` is the full Criterion group name (defaults `join`/`ds`). For `bench run` it is a *prefix* on the auto-generated `{benchmark}/{query}/{ds}/{algo}` identity (defaulting to `run`), so workload identity stays in `target/criterion/{group}/`.
- **CLI join CSV header**: `kermit join` and `kermit bench join --output` prepend a CSV header row built from the head's variable names (via `head_column_names` in `kermit/src/main.rs`). Tests or scripts that parse this output as integer tuples must skip the first non-empty line.
- **CI env vars**: All CI jobs set `RUST_BACKTRACE=1`. Release workflow requires `CARGO_REGISTRY_TOKEN` secret.
- **Error handling**: `kermit-bench` uses `thiserror`, `kermit-ds` uses custom error enums with manual `Display`/`Error` impls, and the CLI binary uses `anyhow::Result`.
- **Const-view rewrite**: `DatabaseEngine::join` calls `kermit_algos::rewrite_atoms` before handing the query to `JoinAlgo::join_iter`. Each `Term::Atom("c<id>")` becomes a fresh variable plus a synthetic unary `Const_c<id>` predicate backed by `SingletonTrieIter`; LFTJ never sees atoms. Adding a new data structure does not require handling atoms, but adding a new `JoinAlgo` impl must tolerate being invoked on the rewritten query (with extra unary body predicates).
- **WatDiv benchmark generation**: the 12 `watdiv-stress-*.yml` files are produced by `scripts/watdiv-preprocess/` (Python). Editing them by hand drifts from the preprocessor; regenerate instead. The integration test at `kermit/tests/watdiv_correctness.rs` loads a committed mini fixture — no Python required at test time. The committed YAMLs embed `c<dict-id>` atoms tied to the dictionary produced by a specific preprocessor run, so the YAMLs, `dict.parquet`, and all per-predicate `*.parquet` files must be regenerated and re-uploaded together — never mix-and-matched across runs, or constant atoms in the YAMLs will point at the wrong rows.
- **WatDiv on-the-fly driver**: the vendored `kermit-rdf/vendor/watdiv` binary's CLI is `-d <model> <scale>`, `-s <model> <data> <max-q-size> <q-count>`, and `-q <model> <query-file> <count> <recurrence>`. **All three modes write only to stdout** — they do NOT write per-template `.txt`/`.sparql`/`.desc` files like the design doc originally implied. `kermit-rdf::driver::invoke` captures stdout and splits `-s`/`-q` output on `#end` lines (see `split_templates` / `split_queries`); the vendored binary emits no `.desc` cardinality sidecars, so `expected/*.csv` is empty for now. Two integration tests cover this: `kermit-rdf/tests/e2e_watdiv.rs` (drives the full pipeline) and `kermit/tests/cli_watdiv_gen.rs` (CLI smoke). Both auto-skip on non-Linux/non-x86_64 hosts and on hosts where bwrap can't construct the `/usr/share/dict/words` bind. On NixOS, run them inside `nix develop` so `LD_LIBRARY_PATH` exposes `libstdc++` to the binary; the flake also pulls in `pkgs.bubblewrap`.
