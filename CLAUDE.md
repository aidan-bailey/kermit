# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Kermit is a Rust library for relational algebra research and benchmarking, built as a platform for a Masters thesis investigating the Leapfrog Triejoin algorithm across different data structures. It is a Cargo workspace with 8 crates. All keys are `usize` (dictionary-encoded). The codebase uses entirely safe Rust with no unsafe blocks. See `ARCHITECTURE.md` for detailed algorithmic descriptions, data flow, and Datalog query processing.

## Priorities

This codebase is a Masters thesis research platform. The following priorities, in order, govern code changes. They override stylistic instincts when they conflict.

1. **Test coverage for every algorithm × index-structure pair.** New algorithms and index structures must extend `define_multiway_join_test_suite!` (see Testing Patterns) so the 12 standard join patterns run against every combination. A change that adds a structure or algorithm without extending the suite will be rejected. Each Layout combination on a DS is a distinct test variant (e.g., `HashTrieSip`, `HashTrieFx`); each must run the 12 standard join patterns under every compatible algorithm. Config and BuildMode dimensions are to be tested via the prescribed (but not-yet-implemented) `define_multiway_join_test_suite_with_config!` and `define_multiway_join_test_suite_for_build_mode!` macros respectively; until the first Config/BuildMode consumer lands, write per-config/per-mode tests by hand. See [`docs/specs/optimization-standard.md`](docs/specs/optimization-standard.md).

2. **Human-readable implementations.** Algorithms in `kermit-algos` and structures in `kermit-ds` should read like the paper that defines them — names match the literature, control flow mirrors the published pseudocode. Optimizations that obscure this need a comment justifying the cost they save.

3. **Per-component documentation.** Every algorithm has `docs/algorithms/<name>.md`; every index structure has `docs/data-structures/<name>.md`. Each doc covers representation/pseudocode, invariants, complexity of the trait methods (`insert`/`seek`/`open`/`up` for tries; `join_iter` for algorithms), and a worked micro-example. These docs are for humans — they complement, not replace, `ARCHITECTURE.md`. See `docs/algorithms/TEMPLATE.md` and `docs/data-structures/TEMPLATE.md` for the skeleton.

4. **Extensibility via recognizable patterns.** Adding a new algorithm or index structure follows the exact recipe in "Extending the System" — same file paths, same trait order, same CLI wiring, same test hook. The recipe is the contract; deviations need justification.

5. **Robustness.** Prefer total functions at public API boundaries. Encode invariants in types where practical (`HeapSize`, `TrieIterator`, `JoinAlgo<DS>` are precedents). Internal panics on broken invariants are acceptable — see the LFTJ `open`-after-`at_end` discipline in Gotchas.

6. **Scope discipline.** When the task is "add/fix algorithm X" or "add/fix index structure Y", do not touch siblings. If you discover a bug in `TreeTrie` while adding `ColumnTrie`, file it separately. Cross-cutting refactors are a separate change with explicit scope. This rule exists because thesis benchmarks compare structures against each other — silently changing a sibling invalidates prior measurements.

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
cargo run -- bench run triangle -i hash-trie -a hash-triejoin      # The hash-trie pairing (pairs only with hash-triejoin)
cargo run -- bench run --all -i all -a all                         # Every benchmark × the 3 valid (structure, algorithm) cells; incompatible pairs are skipped with a stderr note
cargo run -- bench run watdiv-example -i tree-trie -a leapfrog-triejoin --force  # Force-regenerate a generator-driven benchmark
cargo run -- bench gen watdiv --scale 100 --tag dev                # Generate WatDiv benchmark on the fly (imperative)
cargo run -- bench gen lubm --scale 1 --tag dev                    # Generate LUBM benchmark on the fly (imperative)
MIRIFLAGS="-Zmiri-disable-isolation" cargo miri setup && cargo miri test  # Check for UB (flag matches CI)
```

Analysis lives in Python, not Cargo — run these from `python/kermit-lab/`:

```bash
uv sync --group test                                        # create .venv, install deps + test extras
uv run kermit-lab --help                                    # thin CLI over kl.load / kl.plot
uv run kermit-lab <subcommand> bench-runs/*.json --out <path>
uv run pytest                                               # kermit-lab's own tests
```

## Toolchain

Rust **nightly** (pinned in `rust-toolchain.toml`). Required components: clippy, miri, rust-analyzer, rustfmt. The `rustfmt.toml` uses `unstable_features=true` so nightly rustfmt is required.

## Development Environment

A Nix flake provides the recommended dev shell: `nix develop`. It sets up nightly Rust (matching `rust-toolchain.toml`), `git-cliff`, `cargo-expand`, Python 3.13, and configures `MIRIFLAGS` and `RUST_BACKTRACE` to match CI. The shell also exports an `LD_LIBRARY_PATH` covering `libstdc++.so.6` / `libz.so.1` so the wheels (numpy, matplotlib) installed by uv into `python/kermit-lab/.venv/` load on NixOS without manual workarounds.

## CI Checks (PR gate)

All of these must pass: `cargo test`, `cargo clippy` (warnings are errors), `cargo fmt --check`, `cargo doc` (doc warnings are errors), `cargo miri test`.

## Workspace Architecture

```
kermit-iters    → Core iterator traits (LinearIterator, TrieIterator). Zero dependencies.
kermit-derive   → Proc macros (#[derive(IntoTrieIter)]) for iterator boilerplate.
kermit-parser   → Datalog query parser (winnow). Parses "Q(X,Z) :- R(X,Y), S(Y,Z)."
kermit-ds       → Data structures: TreeTrie (pointer-based), ColumnTrie (column-oriented),
                  HashTrie (hash-based, generic over a HashStrategy). TreeTrie/ColumnTrie
                  implement Relation + TrieIterable; HashTrie implements Relation +
                  HashTrieIterable.
kermit-algos    → Join algorithms: LeapfrogJoinIter (binary), LeapfrogTriejoinIter (multi-way),
                  HashTriejoin (hash-based multi-way). Generic over data structures via the
                  JoinAlgo<DS> trait. Also hosts query optimisers (optimiser/ module):
                  QueryOptimiser implementations plan the variable ordering (QueryPlan) that
                  JoinAlgo::join_iter executes.
kermit-bench    → Benchmark definitions, discovery, and caching. No internal deps.
                  YAML-based benchmark declarations (supports multiple named queries per benchmark),
                  ZivaHub download, platform cache dir (~/.cache/kermit/benchmarks/ on Linux).
                  `discovery::load_all_benchmarks_with_cache` reads both workspace
                  `benchmarks/*.yml` and any cache subdir containing both `benchmark.yml`
                  AND `meta.json` (the marker that distinguishes a generator-produced bench).
kermit-rdf      → RDF/SPARQL preprocessing pipelines for on-the-fly benchmark generation.
                  Vendor dirs: `vendor/lubm-uba/lubm-uba.jar` (committed, ~2.9 MB) and
                  `vendor/watdiv/` (binary at `bin/Release/watdiv` is committed, ~360 KB —
                  force-added past the inert `**/watdiv` ignore rule; surrounding
                  `MODEL.txt`/`files/`/`VERSION` are committed too).
                  Two pipelines: `pipeline::run_pipeline` (watdiv-onthefly) and
                  `lubm::pipeline::run_lubm_pipeline` (lubm-onthefly, with Univ-Bench
                  TBox forward chaining via `lubm::entailment`). Both are thin
                  `generator::Generator` impls over the one shared post-driver
                  orchestrator `generator::process_artifacts`; a third generator
                  implements the trait rather than copying the sequence.
                  Shared stages: `generator`, `partition`, `parquet`, `dict`,
                  `sparql::translator`, `yaml_emit`, `expected`.
                  The 14 LUBM queries live at `queries/lubm/q*.sparql`
                  and are exposed via `lubm::queries::lubm_query_specs`.
kermit          → CLI binary (clap). Subcommands: join, bench (join|ds|run|list|fetch|clean|
                  gen [watdiv|lubm]).
                  Provides the two join entry points (db::lftj_join, db::hash_join)
                  over a `BTreeMap<String, R>` relation store; all (structure, algorithm)
                  dispatch goes through execution::ExecutionFamily.
                  All Criterion execution lives here (including SpaceMeasurement).
```

**Dependency flow:** `kermit-iters`/`kermit-derive` → `kermit-ds`; `kermit-iters`/`kermit-derive`/`kermit-parser` → `kermit-algos`; `kermit-bench` → `kermit-rdf`; everything → `kermit` (binary). Three crates are leaves with no internal deps: `kermit-iters`, `kermit-parser`, `kermit-bench`. `kermit-parser` is consumed by `kermit-algos` only. `kermit-algos` pulls `kermit-ds` in as a dev-dependency only, so there is no production edge between them.

> `kermit-rdf` depends on `kermit-bench` **only** — deliberately. Its link to the query layer is textual, not structural: `sparql::translator` emits a Datalog *string* into the generated `benchmark.yml`, parsed by `kermit-parser` later (in the binary, at bench-run time). It never constructs a relation, so it needs no `kermit-ds`. Don't re-add either dependency to satisfy a type — if you find yourself wanting one, the pipeline boundary has probably moved.

## Key Trait Hierarchy

- **JoinIterable** (marker) → **LinearIterable** → **LinearIterator** (`key`, `next`, `seek`, `at_end`)
- **JoinIterable** (marker) → **TrieIterable** → **TrieIterator** : LinearIterator + `open`, `up`
- **JoinIterable** (marker) → **HashTrieIterable** → **HashTrieIterator** (`u64` hash keys, exact-match `lookup`, plus `size`, `open`, `up`, `leaf_tuples`) — a **parallel family, not a `TrieIterator` subtrait**. `LinearIterator::seek` is least-upper-bound and needs sorted data; hash navigation is exact-match, so the contracts cannot merge (rationale in `kermit-iters/src/hash_trie.rs`, citing SIGMOD 2020). The fork propagates up the whole stack — separate algorithm, separate join entry point (`lftj_join` vs `hash_join`, sharing one body via the `JoinFamily` trait), separate test suites — and leaves exactly 3 valid `(structure, algorithm)` pairs.
- **Relation**: JoinIterable + Projectable — core data abstraction (`new`, `from_tuples`, `insert`, `insert_all`, `header`)
- **JoinAlgo\<DS\>**: algorithm trait decoupled from data structures
- **HeapSize**: heap-allocated byte count for space benchmarking (`heap_size_bytes()`)
- **QueryOptimiser**: plans a `QueryPlan` (the LFTJ global attribute order) from a query + `CatalogStats`; consumed by `JoinAlgo::join_iter`. Implementations: `LexicographicOptimiser` (default), `CardinalityOptimiser`.
- **Cardinality**: stored-tuple count for optimiser statistics (`tuple_count()`; `HashTrie` counts multiset size)

## Testing Patterns

Tests use macro-generated suites that combinatorially test all data structures against all algorithms:
- `define_multiway_join_test!()` — individual parametrized test
- `define_multiway_join_test_suite!()` — generates 12 standard join patterns (unary, triangle, chain, star, self-join, existential, empty-result, single-relation, four-way-chain, wide-fanout, dead-end, deep-dead-end)
- `relation_trie_test_suite!()` / `parquet_test_suite!()` (`kermit-ds/tests/common/macros.rs`) — the layer below the join for `TrieIterable` structures: iterator contract (traversal + seek), construction, and Parquet round-trip
- `hash_trie_test_suite!(Type, Strategy)` / `parquet_test_suite!(Type, collector)` — the same layer for `HashTrieIterable` structures (`open`/`next`/`lookup`/`up`/`size`/`leaf_tuples`; round-trips via `collect_tuples()`). The two iterator families are deliberately separate traits, so a hash-family structure cannot reuse the sorted-trie suite — invoke the hash-family one per Layout alias instead (see `kermit-ds/tests/hash_trie_tests.rs`)
- Uses `paste!` crate for macro hygiene

Unit tests live inline in `#[cfg(test)]` blocks. Integration tests in `tests/` directories.

## Extending the System

These recipes are the recognizable pattern referenced in Priorities item 4. Follow them exactly when possible; the consistency itself is the deliverable.

### Adding a new index structure

1. **Module layout.** Create `kermit-ds/src/ds/<name>/` containing `mod.rs`, `implementation.rs`, and `<name>_iter.rs`. Existing precedents: `kermit-ds/src/ds/tree_trie/` (pointer-based), `kermit-ds/src/ds/column_trie/` (column-oriented).
2. **Implement `Relation` + `Projectable` + `HeapSize`** for the structure in `implementation.rs`. `HeapSize::heap_size_bytes()` returns *only* heap-allocated bytes (not `size_of::<Self>`). See `kermit-ds/src/ds/tree_trie/implementation.rs`.
3. **Implement `TrieIterator`** for the iter type in `<name>_iter.rs`. Apply `#[derive(IntoTrieIter)]` from `kermit-derive` so the `IntoIterator` + `TrieIteratorWrapper` bridge is generated for you.
4. **Implement `TrieIterable`** for the structure (wires `trie_iter()` to your iter type). The LFTJ `open`-after-`at_end` discipline is load-bearing — see the LFTJ gotcha and the `feedback_lftj_open_after_at_end` memory.
   For a **hash-family** structure, implement `HashTrieIterator` + `HashTrieIterable` instead (`#[derive(IntoTrieIter)]` does not apply). Be aware that the shared helpers `project_via_trie_iter` and `TrieIteratorWrapper` are `TrieIterable`-only, so you must supply your own equivalents, and that the test suites fork too — reach for `hash_trie_test_suite!`, not `relation_trie_test_suite!`; see the test-macro gotcha.
5. **Register the module.** Add `mod <name>;` and `pub use <name>::<Type>;` in `kermit-ds/src/ds/mod.rs`, plus a variant on the `IndexStructure` enum in that file.
6. **Wire the CLI.** Add a variant to `IndexStructureSelector` in `kermit/src/main.rs` and to its `expand()` method. For a sorted (`TrieIterable`) structure, add a `SortedTrie` variant plus a `SortedTrieRelation` impl in `kermit/src/execution.rs` and a match arm in `Execution::for_pair`; then add arms in `dispatch_run_bench` and `dispatch_ds_bench` in `main.rs`. A structure in a new trait family needs its own `ExecutionFamily` impl.
7. **Wire the tests.** Add a `define_multiway_join_test_suite!(<Type>, LeapfrogTriejoin, LexicographicOptimiser);` invocation in `kermit/tests/join_tests.rs` so the 12 standard join patterns run against the structure with every algorithm (Priorities item 1); add a second invocation with `CardinalityOptimiser` so both optimisers are covered.
8. **Write the doc.** Create `docs/data-structures/<name>.md` from `docs/data-structures/TEMPLATE.md` (Priorities item 3).

Do **not** modify other index structures during this work (Priorities item 6).

### Adding a new join algorithm

1. **Module layout.** Create `kermit-algos/src/<name>.rs`. Existing precedents: `kermit-algos/src/leapfrog_join.rs` (binary intersection, internal helper) and `kermit-algos/src/leapfrog_triejoin.rs` (multi-way join, the CLI-exposed entry point).
2. **Implement `JoinAlgo<DS>`**, narrowing `DS` to the iterator family you actually traverse — `TrieIterable` (as `LeapfrogTriejoin` does) or `HashTrieIterable` (as `HashTriejoin` does). The trait's own bound is the weak `JoinIterable` marker. The algorithm must tolerate const-rewritten queries (extra unary `Const_c<id>` body predicates from `kermit_algos::rewrite_atoms`) — see the const-view-rewrite gotcha — and must validate the incoming `QueryPlan`.
3. **Register the module.** Add `mod <name>;` and `pub use <name>::<Type>;` in `kermit-algos/src/lib.rs`, plus a variant on the `JoinAlgorithm` enum in that file.
4. **Wire the CLI.** Add a variant to `JoinAlgorithmSelector` in `kermit/src/main.rs` and to its `expand()` method, then add the algorithm's valid cells to `Execution` / `Execution::for_pair` in `kermit/src/execution.rs` (the compiler flags the incomplete match) and a `dispatch_run_bench` arm. `kermit join` / `bench join` dispatch through the same `Execution` cells (`load_query_runner` in `main.rs`), so a cell added there is reachable from every command. A sorted-family algorithm plugs into `lftj_join<R, JA>` (generic in the algorithm); a hash-family one needs its own entry point beside `hash_join`, which hardwires `HashTriejoin` — both are thin wrappers over the shared private body in `kermit/src/db.rs`, parameterised by the `JoinFamily` trait.
5. **Wire the tests.** Existing index structures pick up your algorithm combinatorially via `define_multiway_join_test_suite!` — add a fresh invocation per index structure in `kermit/tests/join_tests.rs`, e.g. `define_multiway_join_test_suite!(<DS>, <YourAlgo>, LexicographicOptimiser);` plus a second invocation with `CardinalityOptimiser` (Priorities item 1).
6. **Write the doc.** Create `docs/algorithms/<name>.md` from `docs/algorithms/TEMPLATE.md` (Priorities item 3).

Do **not** modify other algorithms during this work (Priorities item 6).

### Adding a new query optimiser

1. **Module layout.** Create `kermit-algos/src/optimiser/<name>.rs`. Existing precedents: `lexicographic.rs` (stats-free default) and `cardinality.rs` (smallest-relation-first).
2. **Implement `QueryOptimiser`.** Build the ordering with `topological_order(num_vars, predicate_variables, rank)` from the shared `ordering` module — ranking only chooses among Kahn-ready variables, so your plan is valid by construction. Consume statistics via `CatalogStats`; treat missing entries as "assume large". Determinism (canonical-index tie-break) comes from `topological_order` itself.
3. **Register the module.** Add `mod <name>;` and the re-export in `kermit-algos/src/optimiser/mod.rs`, plus the crate-root re-export in `kermit-algos/src/lib.rs`.
4. **Wire the CLI.** Add a variant to the `Optimiser` enum in `kermit-algos/src/lib.rs` (with `instantiate()` and `axis_value()` arms; the `axis_values_match_clap_value_names` guard test pins axis naming). The `--optimiser` flag on `join`, `bench join`, and `bench run` picks it up via `ValueEnum`.
5. **Wire the tests.** Two hooks, both required. *Breadth:* add a `define_multiway_join_test_suite!(<DS>, <Algo>, <YourOptimiser>);` invocation per valid (DS, algorithm) pair in `kermit/tests/join_tests.rs` (Priorities item 1), plus unit tests for the ranking itself in your module. *Depth:* add a row to the `optimisers` vec in `kermit/tests/lubm_cardinalities.rs` so the policy is checked against the published LUBM(1, 0) reference cardinalities (needs `java` on PATH, so run it inside `nix develop` — it skips silently otherwise). The breadth suite is combinatorial but its 3-5 tuple fixtures never prune deeply enough to fail a trie descent, so it cannot see an executor bug that only some variable orderings expose; `lubm_cardinalities.rs` is the only test with real data and is where such a bug surfaces. Skipping the second hook is how a wrong-answer LFTJ bug survived until 2026-09-08 — see the "A failed descent is atomic" invariant in `docs/algorithms/leapfrog-triejoin.md`.
6. **Write the doc.** Create `docs/optimisers/<name>.md` from `docs/optimisers/TEMPLATE.md` (Priorities item 3).

Do **not** modify other optimisers, algorithms, or index structures during this work (Priorities item 6).

### Adding an optimization to a data structure or algorithm

Optimizations fall into one of three categories — Layout, Config, or BuildMode.
Each has a prescribed Rust shape, CLI surface, bench-axis namespace, and test
obligation. The full recipe lives in
[`docs/specs/optimization-standard.md`](docs/specs/optimization-standard.md);
the short version:

1. Classify the optimization into Layout (compile-time type parameter), Config
   (runtime flag), or BuildMode (construction-time choice).
2. Define the relevant Rust types implementing `LayoutOption`/`ConfigOption`/
   `BuildMode` from `kermit_iters::optimization`.
3. Extend the DS or algorithm's `HasOptimizationAxes` impl with the new
   axis under the right prefix (`ds_layout_*` / `ds_config_*` / `ds_build_mode`).
4. Add CLI surface (`--ds-layout-<dim>` / `--ds-config <flag>=<value>` /
   `--ds-build <mode>`).
5. Extend the test suite per the standard (type aliases for Layout, new
   macro invocations for Config/BuildMode).
6. Update the DS or algorithm's per-component doc with an "Optimizations"
   subsection.

Deviations from this recipe need explicit justification, per Priorities item 6.

### Adding a new benchmark

Add a YAML file in `benchmarks/`. For static benchmarks: declare `relations` (with download URLs) and Datalog `queries` (see `benchmarks/triangle.yml`). For declarative generators: declare a `generator: { kind: watdiv|lubm, scale: N, ... }` block — `bench run <name>` will materialise the data on demand via `kermit-rdf` (see `benchmarks/README.md` and `kermit/src/materialize.rs::materialize`).

## Benchmark Reference Docs

- `docs/benchmarks/LUBM.md` — LUBM usage, the 14 queries with reference cardinalities, entailment rule set, scale ceiling.
- `docs/benchmarks/WATDIV.md` — WatDiv usage, stress-template parameters, vendoring rules, non-determinism discipline.
- `kermit-rdf/src/lubm/README.md` — module-internal contributor doc for the LUBM pipeline.

## Component Reference Docs

Per Priorities item 3, every algorithm and index structure has a dedicated doc.

- `docs/algorithms/leapfrog-triejoin.md` — multi-way worst-case-optimal join (CLI-exposed).
- `docs/algorithms/leapfrog-join.md` — k-way sorted intersection used internally by `LeapfrogTriejoin`.
- `docs/algorithms/hash-triejoin.md` — hash-trie-based multi-way join (`-a hash-triejoin`).
- `docs/data-structures/tree-trie.md` — pointer-based trie (`-i tree-trie`).
- `docs/data-structures/column-trie.md` — column-oriented trie (`-i column-trie`).
- `docs/data-structures/hash-trie.md` — hash-based trie (`-i hash-trie`).
- `docs/algorithms/TEMPLATE.md`, `docs/data-structures/TEMPLATE.md` — skeletons for new component docs.
- `docs/optimisers/lexicographic.md` — default variable-ordering policy (`--optimiser lexicographic`).
- `docs/optimisers/cardinality.md` — smallest-relation-first policy (`--optimiser cardinality`).
- `docs/optimisers/TEMPLATE.md` — skeleton for new optimiser docs.

## Code Style

- `rustfmt.toml` is extensively configured: `max_width=100`, `trailing_comma="Vertical"`, `imports_granularity="One"`, `group_imports="StdExternalCrate"`, `match_arm_leading_pipes="Always"`.
- Always run `cargo fmt --all` before committing — nightly rustfmt required due to unstable features.

## Gotchas

- **Miri isolation**: CI runs miri with `MIRIFLAGS="-Zmiri-disable-isolation"` and excludes `kermit` and `kermit-bench` from miri tests (Criterion and network code). Use the same flag locally or tests may fail differently. Miri also can't model `fchmod`, so tests using `std::fs::set_permissions` or `std::fs::copy` need `#[cfg_attr(miri, ignore = "...")]` (the kermit-rdf driver fs tests are gated this way).
- **git-cliff**: `cliff.toml` configures changelog generation via [git-cliff](https://git-cliff.org/). The release workflow auto-generates changelogs from conventional commits.
- **NEVER run `cargo fmt` outside `nix develop`**: `rustfmt.toml` uses nightly-only settings, and stable rustfmt rewrites ~30+ files (collapses match patterns, expands single-line fns) instead of just printing warnings. Use `nix develop --command cargo fmt --all`. `cargo +nightly fmt --all` works only with rustup nightly (NixOS hosts typically don't have it).
- **Space benchmarks**: `kermit/src/measurement.rs` contains `SpaceMeasurement` (custom Criterion `Measurement`) and `BytesFormatter`. Both `bench ds --metrics space` and `bench run --metrics space` route through `Criterion<SpaceMeasurement>` via `iter_custom`, producing `target/criterion/` output alongside the time metrics. The closure calls `heap_size_bytes()` per iter on the pre-built relation (wrapped in `std::hint::black_box` to defeat LICM); result is deterministic — per-iter mean equals `heap_size_bytes()` exactly. **Trap:** `iter_custom` calibration uses wall-clock during warmup even for non-time `Measurement`s; a near-instant closure makes Criterion ramp `iters` toward `u64::MAX` and `iters * bytes` arithmetic saturates `usize`. Keep at least one O(N) call inside the loop; don't precompute and multiply.
- **No Criterion auto-plots**: `kermit/Cargo.toml` opts out of Criterion's default features (`default-features = false, features = ["rayon", "cargo_bench_support"]`) so the `plotters` dep is excluded entirely. Result: no SVG/HTML rendering, and the zero-variance panic that `SpaceMeasurement` used to trigger no longer applies. Measurement JSON (`estimates.json`, `sample.json`, `benchmark.json`, `tukey.json`) is still written per-function under `target/criterion/{group}/{directory_name}/{base,new}/`. Analysis and plotting live in `python/kermit-lab/` (uv-managed notebook-first library: `kl.load()` returns a pandas DataFrame including the `ds_*`/`algo_*` optimization axes; `kl.plot(df, kind=…, x=…, colour=…, facet=…)` is the general engine and `kl.scaling()`/`kl.bar_time()`/`kl.ablation()`/etc. are presets over it, each returning `matplotlib.figure.Figure`; `kl.summary`/`compare`/`bootstrap_ratio_ci`/`mannwhitney_u` for pivots/stats). The CLI is a thin wrapper. See `docs/specs/2026-05-04-remove-criterion-graphs-design.md`.
- **JSON bench reports**: every `kermit bench` invocation writes a machine-readable report. Default path is `bench-runs/{kind}-{unix-millis}.json` (`bench-runs/` is auto-created and gitignored at the workspace root); `--report-json <PATH>` overrides. Output is always a JSON array of `BenchReport` objects (one per query for `bench run`, exactly one for `bench join` / `bench ds`). Each object carries `metadata` (label/value pairs mirroring stderr), `axes` (a `BTreeMap<String, serde_json::Value>` of structured axis values for tooling — conventional keys: `data_structure`, `algorithm`, `optimiser`, `query`, `benchmark`, `relation_path`, `relation_bytes`, `tuples`, `arity`, `relations`), and `criterion_groups` pointers resolving to `target/criterion/{group}/{directory_name}/`. The on-disk `directory_name` replaces `/` in `function_id` with `_` — read it from each subdir's `benchmark.json:directory_name` rather than computing it. Schema is versioned via `schema_version` (currently `2`) and lives in `kermit/src/bench_report.rs`; full key catalogue in `docs/specs/bench-report-schema.md`. Bump the version on any breaking field-name or value-type change.
- **bench `--name` semantics**: For `bench join` and `bench ds`, `--name` is the full Criterion group name (defaults `join`/`ds`). For `bench run` it is a *prefix* on the auto-generated `{benchmark}/{query}/{ds}/{algo}` identity (defaulting to `run`), so workload identity stays in `target/criterion/{group}/`.
- **`bench run` sweeps are cells, not pairs**: `kermit/src/execution.rs` defines `Execution` — an enum whose variants *are* the three valid (structure, algorithm) cells (`TrieLftj(TreeTrie|ColumnTrie)`, `HashHtj(hasher)`). `Sweep::expand` turns the `-i`/`-a` selectors into cells and a `skipped` list; `-i all -a all` runs exactly the 3 valid cells (skips announced on stderr), and a single explicit incompatible pair is a usage error. One generic `run_benchmark<F: ExecutionFamily>` in `main.rs` serves both trait families; the report's `data_structure`/`algorithm` axes come from `F::execution()`, so a report cannot name an algorithm it did not run (issue #56). `bench ds` uses the same shape: `Execution::for_structure` (total, since each structure has one compatible algorithm) picks the cell and one generic `run_ds_bench<F: RelationFamily>` serves both (issue #61). `RelationFamily` is the relation-facing supertrait of `ExecutionFamily`; `bench ds` uses the structure-only markers `SortedTrieFamily<R>` / `HashTrieFamily<H>`, which cannot join, and the join families embed and delegate to them.
- **Two iterator families, two test-macro families**: `relation_trie_test_suite!`, `trie_traversal_tests!` and `trie_seek_tests!` (`kermit-ds/tests/common/macros.rs`) all bound on `TrieIterable`/`TrieIterator`, so a hash-family structure cannot be plugged into them. Use `hash_trie_test_suite!(Type, Strategy)` instead — `kermit-ds/tests/hash_trie_tests.rs` covers `HashTrieSip`/`HashTrieFx` — together with the `collect_tuples()`-driven `parquet_test_suite!(Type, collector)` arm. Adding a hash-family structure means invoking the hash-family suite once per Layout alias, not extending the sorted-trie macros.
- **CLI join CSV header**: `kermit join` and `kermit bench join --output` prepend a CSV header row built from the head's variable names (via `head_column_names` in `kermit/src/main.rs`). Tests or scripts that parse this output as integer tuples must skip the first non-empty line.
- **CI env vars**: All CI jobs set `RUST_BACKTRACE=1`. Release workflow requires `CARGO_REGISTRY_TOKEN` secret.
- **Error handling**: `kermit-bench` uses `thiserror`, `kermit-ds` uses custom error enums with manual `Display`/`Error` impls, and the CLI binary uses `anyhow::Result`.
- **Const-view rewrite**: `lftj_join` / `hash_join` (via their shared body in `kermit/src/db.rs`) call `kermit_algos::rewrite_atoms` before handing the query to `JoinAlgo::join_iter`. Each `Term::Atom("c<id>")` becomes a fresh variable plus a synthetic unary `Const_c<id>` predicate backed by `SingletonTrieIter`; LFTJ never sees atoms. Adding a new data structure does not require handling atoms, but adding a new `JoinAlgo` impl must tolerate being invoked on the rewritten query (with extra unary body predicates).
- **WatDiv benchmark generation**: the 12 committed static `watdiv-stress-{100,1000}-*.yml` snapshots are produced by `scripts/watdiv-preprocess/` (Python) — distinct from the declarative-generator `watdiv-stress-default.yml`, which carries a `generator:` block instead. The Rust on-the-fly path `bench gen watdiv` (next gotcha) supersedes this for new generation; the Python pipeline is retained for regenerating the committed snapshots if their dictionary/parquet bundle ever needs to change. Editing committed YAMLs by hand drifts from the preprocessor; regenerate instead. The integration test at `kermit/tests/watdiv_correctness.rs` loads a committed mini fixture — no Python required at test time. The committed YAMLs embed `c<dict-id>` atoms tied to the dictionary produced by a specific preprocessor run, so the YAMLs, `dict.parquet`, and all per-predicate `*.parquet` files must be regenerated and re-uploaded together — never mix-and-matched across runs, or constant atoms in the YAMLs will point at the wrong rows. User-facing usage: `docs/benchmarks/WATDIV.md`.
- **WatDiv on-the-fly driver**: the vendored `kermit-rdf/vendor/watdiv` binary's CLI is `-d <model> <scale>`, `-s <model> <data> <max-q-size> <q-count>`, and `-q <model> <query-file> <count> <recurrence>`. **All three modes write only to stdout** — they do NOT write per-template `.txt`/`.sparql`/`.desc` files like the design doc originally implied. `kermit-rdf::driver::invoke` captures stdout and splits `-s`/`-q` output on `#end` lines (see `split_templates` / `split_queries`); the vendored binary emits no `.desc` cardinality sidecars, so `expected/*.csv` is empty for now. Two integration tests cover this: `kermit-rdf/tests/e2e_watdiv.rs` (drives the full pipeline) and `kermit/tests/cli_watdiv_gen.rs` (CLI smoke). Both auto-skip on non-Linux/non-x86_64 hosts and on hosts where bwrap can't construct the `/usr/share/dict/words` bind. On NixOS, run them inside `nix develop` so `LD_LIBRARY_PATH` exposes `libstdc++` to the binary; the flake also pulls in `pkgs.bubblewrap`.
- **LUBM on-the-fly generation**: `bench gen lubm --scale N --tag STR` invokes the vendored `kermit-rdf/vendor/lubm-uba/lubm-uba.jar` with `-f NTRIPLES --consolidate Maximal --compress`, gunzips the resulting `Universities.nt.gz`, then runs Univ-Bench TBox forward chaining (`kermit-rdf::lubm::entailment`) before partitioning. JDK 8 must be on PATH — the flake provides `pkgs.jdk8`; CI runners without flake need `apt-get install openjdk-8-jre` or equivalent. The jar's CLI emits two document-self triples (`<>` subject) per file that strict N-Triples parsers reject, so `lubm/driver::gunzip` line-filters them at extraction time. The 14 LUBM queries are committed verbatim at `kermit-rdf/queries/lubm/q1.sparql … q14.sparql` (paper Appendix A), embedded via `include_str!` in `lubm/queries.rs`, and exposed with LUBM(1, 0) reference cardinalities (paper Table 3). The entailment rule list in `lubm/entailment.rs` is hardcoded to Univ-Bench (subClassOf, subPropertyOf, owl:TransitiveProperty for `subOrganizationOf`, owl:inverseOf for `hasAlumnus`/`degreeFrom`, plus realisation of `Chair` from `headOf`); not a general OWL reasoner. The bit-identity invariant of `lubm-uba-rs` only applies *upstream* — kermit's pipeline rewrites the data through entailment, so the resulting `data.entailed.nt` is not byte-identical to anything the original UBA produces. SHA-256 of the jar is recorded in `meta.json` for provenance.
- **LUBM jar regeneration**: see `kermit-rdf/vendor/lubm-uba/REGENERATE.md`. The currently vendored jar SHA-256 is the authoritative pin; rebuilding under a different JDK may shift bytes. The lubm-uba-rs `pom.xml` enforces Java 1.7 source/target, but JDK 8 is what we build with (per the lubm-uba-rs flake).
- **Declarative generator YAMLs**: a `benchmarks/<name>.yml` may declare `generator: { kind: watdiv|lubm, scale: N, ... }` instead of `relations`/`queries`. On `bench run <name>`, `kermit/src/materialize.rs::materialize` is called between `resolve_benchmarks` and `run_benchmark`. It hashes the spec (`GeneratorSpec::spec_hash` in `kermit-bench`), compares against the cached `meta.json.spec_hash`, and either short-circuits on a hit, errors with `BenchError::SpecDrift` on a mismatch, or invokes the underlying `kermit-rdf` pipeline. **Spec drift never auto-regenerates** — the user must pass `bench run --force <name>` to opt into wiping the cache subdir and rebuilding (a watdiv-scale-1000 run is multi-minute work). Legacy `meta.json` files without a `spec_hash` field (schema_version=1) are treated as drift, also requiring `--force`. The imperative `bench gen watdiv|lubm` CLIs are unchanged: they keep their CLI-flag-only surface, do not accept YAML names, and write into the same cache layout. `PipelineMeta` and `LubmMeta` schema bumped to `2` to add the optional `spec_hash` field.
- **`bench list` status display**: now distinguishes `cached` / `not cached` (static benchmarks) from `not generated` / `cached` / `stale` (generator benchmarks). The status fn lives at `kermit/src/main.rs::describe_benchmark_status`.
- **Cache-side `benchmark.yml` strips `generator:`**: `kermit_rdf::yaml_emit::write_benchmark_yaml` writes `generator: None` to the cache (provenance lives in `meta.json.spec_hash`). After `load_all_benchmarks_with_cache` overrides on name collision, the merged def has `generator: None` even when the workspace YAML had a generator block. Code reasoning about provenance must consult the workspace YAML directly via `load_all_benchmarks(&root)` — see `describe_benchmark_status` in `kermit/src/main.rs`.
- **Doc-comment edits and CI**: `///`/`//!` text is linted by clippy `doc_markdown` under CI's `-Dwarnings` — backtick code-like identifiers (`CamelCase`, `snake_case`, paths) or `cargo clippy` fails (rustdoc won't catch it). Comment-/doc-only changes can't break compilation or runtime tests, so verify them with `cargo doc` (intra-doc links) + `cargo clippy`, not the full suite.
