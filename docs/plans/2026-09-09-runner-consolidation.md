# Runner Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One generic Criterion runner for every measured join, fed by a `Workload` value that `bench run` and `bench join` both build, living in a `kermit/src/bench/` module; `bench join` gains the full metric set and optimisation axes.

**Architecture:** Spec `docs/specs/2026-09-09-runner-consolidation-design.md`. `Workload { name, relation_paths, queries }` replaces `run_benchmark`'s `BenchmarkDefinition` + query-filter inputs. The runners, dispatchers, `Metric`, and the Criterion helpers move verbatim from `main.rs` into `bench/{mod,workload,run,ds}.rs`; `main.rs` keeps clap types, thin handlers, and the one-shot `kermit join`. `bench join` then becomes a `Workload::adhoc` + `dispatch_run_bench` handler with group `{prefix}/adhoc/{query-stem}/{ds}/{algo}`.

**Tech Stack:** Rust nightly, clap, Criterion 0.8, serde_json, tempfile (dev). Run every `cargo fmt` inside `nix develop` — stable rustfmt rewrites the tree. Run cargo in the foreground with `CARGO_BUILD_JOBS=2`.

**Conventions:** `rustfmt.toml`: `match_arm_leading_pipes = "Always"` (every arm starts `| `), `imports_granularity = "One"` (one `use { ... };` per file), `max_width = 100`, comments wrap at 80. Clippy `doc_markdown` under `-Dwarnings`: backtick identifiers in doc comments. Commit messages are conventional commits ending with the session's attribution trailer. Never amend; plain commits.

**Ordering constraint:** each task leaves the tree compiling with the full `kermit` test suite green. Tasks 1→5 are sequential.

---

## File map

| File | Task | Change |
| --- | --- | --- |
| `kermit/src/bench/mod.rs` | 1, 2 | new: module root; `Metric`, Criterion helpers, `add_space_bench` |
| `kermit/src/bench/workload.rs` | 1 | new: `Workload`, `NamedQuery`, constructors, unit tests |
| `kermit/src/bench/ds.rs` | 2 | new: `run_ds_bench`, `dispatch_ds_bench` (moved) |
| `kermit/src/bench/run.rs` | 3 | new: `run_benchmark` (now on `&Workload`), `dispatch_run_bench`, `resolve_sweep` (moved) |
| `kermit/src/main.rs` | 2, 3, 4 | shrink: remove moved code; handlers call into `bench` |
| `kermit/tests/cli_join_tests.rs` | 4 | re-point two `bench join` tests at the new naming |
| `kermit/tests/common/cli.rs` | 4 | add `bench_join` helper |
| `kermit/tests/cli_bench_join_axes.rs` | 4 | new CLI test: `bench join` reports `ds_*` axes and all metrics |
| `USAGE.md`, `BENCHMARKING.md`, `CLAUDE.md`, `docs/specs/benchmarking-architecture.md`, `docs/specs/bench-report-schema.md` | 5 | describe the new `bench join` |

---

### Task 1: `Workload` and the `bench` module skeleton

**Files:**
- Create: `kermit/src/bench/mod.rs`
- Create: `kermit/src/bench/workload.rs`
- Modify: `kermit/src/main.rs` (add `mod bench;`)

- [ ] **Step 1: Create the module root**

`kermit/src/bench/mod.rs`:

```rust
//! The `bench` subcommands' measurement layer: what runs under Criterion
//! once the CLI has resolved *which* structure, algorithm and workload.
//!
//! `workload` is the value both `bench run` (from a `benchmarks/*.yml`
//! definition) and `bench join` (from `--relations` / `--query`) hand to
//! the one generic runner in `run`; `ds` is the structure-only runner
//! behind `bench ds`. `main.rs` keeps the clap types and the thin handlers
//! that build these inputs and write the report.

// Task 3 wires `Workload` into the runners; until then it is unused.
#[allow(dead_code)]
pub mod workload;

pub use workload::{NamedQuery, Workload};
```

- [ ] **Step 2: Write the failing unit tests**

`kermit/src/bench/workload.rs` (tests first; the types come in Step 4):

```rust
//! The unit of work a measured join runs: a named set of relation files
//! plus named, already-parsed queries.

use {
    kermit_algos::JoinQuery,
    kermit_bench::BenchmarkDefinition,
    std::path::{Path, PathBuf},
};

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_bench::{QueryDefinition, RelationSource},
        std::fs,
    };

    /// A workspace root holding `data/edge.csv`, plus a definition whose
    /// single relation points at it via `path:` so `ensure_cached` never
    /// touches the network.
    fn local_def() -> (tempfile::TempDir, BenchmarkDefinition) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("data")).unwrap();
        fs::write(root.path().join("data/edge.csv"), "1,2\n2,3\n").unwrap();
        let def = BenchmarkDefinition {
            name: "local".to_string(),
            description: "local fixture".to_string(),
            relations: vec![RelationSource {
                name: "edge".to_string(),
                url: None,
                path: Some("data/edge.csv".to_string()),
            }],
            queries: vec![
                QueryDefinition {
                    name: "pair".to_string(),
                    description: "pair".to_string(),
                    query: "Q(X, Y) :- edge(X, Y).".to_string(),
                },
                QueryDefinition {
                    name: "path".to_string(),
                    description: "path".to_string(),
                    query: "Q(X, Z) :- edge(X, Y), edge(Y, Z).".to_string(),
                },
            ],
            generator: None,
        };
        (root, def)
    }

    #[test]
    fn from_definition_keeps_every_query_in_order() {
        let (root, def) = local_def();
        let w = Workload::from_definition(&def, root.path(), None).unwrap();
        assert_eq!(w.name, "local");
        assert_eq!(w.relation_paths, vec![root.path().join("data/edge.csv")]);
        let names: Vec<&str> = w.queries.iter().map(|q| q.name.as_str()).collect();
        assert_eq!(names, ["pair", "path"]);
    }

    #[test]
    fn from_definition_filter_keeps_exactly_one() {
        let (root, def) = local_def();
        let w = Workload::from_definition(&def, root.path(), Some("path")).unwrap();
        assert_eq!(w.queries.len(), 1);
        assert_eq!(w.queries[0].name, "path");
    }

    #[test]
    fn from_definition_unknown_filter_lists_available_queries() {
        let (root, def) = local_def();
        let err = Workload::from_definition(&def, root.path(), Some("nope")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("query 'nope' not found in benchmark 'local'"), "{msg}");
        assert!(msg.contains("pair, path"), "{msg}");
    }

    #[test]
    fn adhoc_names_query_by_file_stem() {
        let dir = tempfile::tempdir().unwrap();
        let q = dir.path().join("triangle.dl");
        fs::write(&q, "Q(X, Y, Z) :- r(X, Y), s(Y, Z), t(Z, X).").unwrap();
        let rels = vec![PathBuf::from("/x/r.csv"), PathBuf::from("/x/s.csv")];
        let w = Workload::adhoc(rels.clone(), &q).unwrap();
        assert_eq!(w.name, "adhoc");
        assert_eq!(w.relation_paths, rels);
        assert_eq!(w.queries.len(), 1);
        assert_eq!(w.queries[0].name, "triangle");
    }

    #[test]
    fn adhoc_rejects_query_path_without_stem() {
        let err = Workload::adhoc(vec![], Path::new("/")).unwrap_err();
        assert!(err.to_string().contains("query path"), "{err}");
    }
}
```

Field names verified against `kermit-bench/src/definition.rs`: `RelationSource { name, url: Option<String>, path: Option<String> }`, `QueryDefinition { name, description, query }`, `BenchmarkDefinition { name, description, relations, queries, generator: Option<GeneratorSpec> }`. All are re-exported from the `kermit_bench` crate root.

- [ ] **Step 3: Register the module and confirm the tests fail**

In `kermit/src/main.rs`, after `mod bench_report;` add `mod bench;`.

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit bench::workload`
Expected: compile errors — `Workload` / `NamedQuery` not found.

- [ ] **Step 4: Implement**

Insert between the `use` block and `#[cfg(test)]` in `workload.rs`:

```rust
/// One query of a workload, already parsed.
pub struct NamedQuery {
    /// The name that becomes the report's `query` axis and the
    /// `{query}` segment of the Criterion group.
    pub name: String,
    pub query: JoinQuery,
}

/// What the generic runner measures: relation files (already cached or
/// resolved, loaded by the execution family) and the queries to run over
/// them.
pub struct Workload {
    /// The benchmark name (the report's `benchmark` axis), or
    /// [`Workload::ADHOC`] for a `bench join` invocation.
    pub name: String,
    pub relation_paths: Vec<PathBuf>,
    pub queries: Vec<NamedQuery>,
}

impl Workload {
    /// The `name` of a workload built by [`Workload::adhoc`].
    pub const ADHOC: &'static str = "adhoc";

    /// Builds the workload for `bench run`: caches the definition's
    /// relations (downloading `url:` ones on first use), parses every
    /// query string, and keeps only `query_filter` when given.
    ///
    /// # Errors
    ///
    /// Unknown filter name (the message lists the available names), a
    /// relation that cannot be cached, or a query string that fails to
    /// parse.
    pub fn from_definition(
        def: &BenchmarkDefinition, workspace_root: &Path, query_filter: Option<&str>,
    ) -> anyhow::Result<Self> {
        let selected: Vec<&kermit_bench::QueryDefinition> = match query_filter {
            | Some(name) => {
                let q = def.queries.iter().find(|q| q.name == name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "query '{}' not found in benchmark '{}' (available: {})",
                        name,
                        def.name,
                        def.queries
                            .iter()
                            .map(|q| q.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?;
                vec![q]
            },
            | None => def.queries.iter().collect(),
        };
        let relation_paths = kermit_bench::cache::ensure_cached(def, workspace_root)
            .map_err(|e| anyhow::anyhow!("Failed to fetch benchmark data: {e}"))?;
        let queries = selected
            .into_iter()
            .map(|q| {
                let query: JoinQuery = q
                    .query
                    .trim()
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Failed to parse query '{}': {e}", q.query))?;
                Ok(NamedQuery {
                    name: q.name.clone(),
                    query,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            name: def.name.clone(),
            relation_paths,
            queries,
        })
    }

    /// Builds the workload for `bench join`: the given relation paths
    /// verbatim and one query read from `query_path`, named by its file
    /// stem (`triangle.dl` → `triangle`).
    ///
    /// # Errors
    ///
    /// `query_path` has no file stem, cannot be read, or does not parse.
    pub fn adhoc(relation_paths: Vec<PathBuf>, query_path: &Path) -> anyhow::Result<Self> {
        let name = query_path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("query path {query_path:?} has no file name"))?
            .to_string();
        let text = std::fs::read_to_string(query_path)
            .map_err(|e| anyhow::anyhow!("Failed to read query file {query_path:?}: {e}"))?;
        let query: JoinQuery = text
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse query file {query_path:?}: {e}"))?;
        Ok(Self {
            name: Self::ADHOC.to_string(),
            relation_paths,
            queries: vec![NamedQuery {
                name,
                query,
            }],
        })
    }
}
```

Note the error strings for `from_definition` reproduce today's messages in `run_benchmark` exactly (`query '…' not found in benchmark '…' (available: …)`, `Failed to fetch benchmark data: …`, `Failed to parse query '…': …`) so the existing CLI tests that grep for them keep passing.

- [ ] **Step 5: Run the tests**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit bench::workload`
Expected: 5 passed. Then `CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings` clean (the `allow(dead_code)` on the module covers the unused type) and `RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc -p kermit --no-deps` clean.

- [ ] **Step 6: fmt inside `nix develop`, commit**

```bash
nix develop --command cargo fmt --all && nix develop --command cargo fmt --all --check
git add kermit/src/bench kermit/src/main.rs
git commit -m "feat(kermit): add bench::Workload, the runner's input for bench run and bench join"
```

---

### Task 2: Move `Metric`, the Criterion helpers, and the `bench ds` runner

Pure relocation. No behaviour change; every existing test must pass unchanged.

**Files:**
- Modify: `kermit/src/bench/mod.rs`
- Create: `kermit/src/bench/ds.rs`
- Modify: `kermit/src/main.rs`

- [ ] **Step 1: Move `Metric` and the helpers into `bench/mod.rs`**

Cut from `main.rs` and paste into `bench/mod.rs` (below the `pub use`), verbatim:
- the `#[derive(Copy, Clone, Debug, PartialEq, clap::ValueEnum)] enum Metric { … }` with its doc comments — make it `pub enum Metric` (clap needs it public across modules);
- `fn build_time_criterion(args: &BenchArgs) -> criterion::Criterion`;
- `fn build_space_criterion(args: &BenchArgs) -> criterion::Criterion<measurement::SpaceMeasurement>`;
- `fn add_space_bench<R>(…) -> CriterionGroupRef` with its doc comment.

Make the three functions `pub(super)`. Add to the top of `bench/mod.rs`:

```rust
use {
    crate::{bench_report::{CriterionGroupRef, ReportMetric}, measurement, BenchArgs},
    kermit_ds::HeapSize,
    std::time::Duration,
};

pub mod ds;
```

and change the module-doc block to end with `pub mod ds;` / `pub mod workload;` in that order (keep the `#[allow(dead_code)]` on `workload` for now).

- [ ] **Step 2: Move `run_ds_bench` and `dispatch_ds_bench` into `bench/ds.rs`**

Create `kermit/src/bench/ds.rs`:

```rust
//! The `bench ds` runner: one index structure over one relation file,
//! generic over the [`RelationFamily`] so both trait families share it.

use {
    super::{add_space_bench, build_space_criterion, build_time_criterion, Metric},
    crate::{
        bench_report::{write_metadata_block, BenchKind, BenchReport, CriterionGroupRef, MetadataLine, ReportMetric},
        execution::{Execution, HashTrieFamily, RelationFamily, SortedTrie, SortedTrieFamily},
        measurement,
        options::{with_hash_trie_layout, HasherChoice, PruningChoice},
        BenchArgs,
    },
    kermit_ds::{HashTrieConfig, IndexStructure, Relation},
    std::{collections::BTreeMap, fs, io, path::Path},
};
```

then paste, verbatim from `main.rs`, `fn run_ds_bench<F: RelationFamily>(…)` (with its doc comment) and `fn dispatch_ds_bench(…)` (with its doc comment and `#[allow(clippy::too_many_arguments)]`). Make `dispatch_ds_bench` `pub(crate)`; `run_ds_bench` stays private to the module. Delete both from `main.rs`. Let the compiler prune the `use` list: remove anything it reports unused, add anything it reports missing (`Relation` is needed for `.header()`; `fs` for `fs::metadata`).

- [ ] **Step 3: Fix `main.rs` imports**

In `main.rs`'s second `use { … }` block add `bench::{dispatch_ds_bench, Metric},` and remove now-unused items (`HashTrieFamily`, `SortedTrieFamily`, `RelationFamily`, `HeapSize`, `Duration`, `measurement` uses if the compiler flags them — keep `mod measurement;`). `run_ds_bench_command` keeps calling `dispatch_ds_bench(...)` with the same arguments. `options.rs` refers to `crate::IndexStructureSelector` — unchanged.

- [ ] **Step 4: Verify nothing changed**

```bash
CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test -p kermit
```
Expected: clean; every test passes (the `bench ds` CLI suites `cli_join_tests`, `cli_hash_trie_*`, `cli_end_to_end_metric` exercise the moved code). `grep -n 'fn run_ds_bench\|fn dispatch_ds_bench\|enum Metric\|fn add_space_bench' kermit/src/main.rs` must be empty.

- [ ] **Step 5: fmt inside `nix develop`, commit**

```bash
git add kermit/src/bench kermit/src/main.rs
git commit -m "refactor(kermit): move Metric, Criterion helpers and the bench ds runner into bench/"
```

---

### Task 3: Move the `bench run` runner onto `Workload`

**Files:**
- Create: `kermit/src/bench/run.rs`
- Modify: `kermit/src/bench/mod.rs` (add `pub mod run;`, drop the `allow`)
- Modify: `kermit/src/main.rs`

- [ ] **Step 1: Create `bench/run.rs` with the moved code**

```rust
//! The generic runner behind `bench run` and `bench join`: one execution
//! cell over one [`Workload`], every requested metric, one report per
//! query.

use {
    super::{add_space_bench, build_space_criterion, build_time_criterion, Metric, Workload},
    crate::{
        bench_report::{write_metadata_block, BenchKind, BenchReport, CriterionGroupRef, MetadataLine, ReportMetric},
        execution::{Execution, ExecutionFamily, HashHtj, RelationFamily, SortedTrie, Sweep, TrieLftj},
        options::{with_hash_trie_layout, HasherChoice, PruningChoice},
        BenchArgs, IndexStructureSelector, JoinAlgorithmSelector,
    },
    kermit_algos::Optimiser,
    kermit_ds::{HashTrieConfig, Relation},
    std::{collections::BTreeMap, io},
};
```

Paste `fn run_benchmark`, `fn dispatch_run_bench`, and `fn resolve_sweep` from `main.rs` (each with its doc comment), then apply exactly these edits:

1. `run_benchmark` signature becomes
   ```rust
   fn run_benchmark<F: ExecutionFamily>(
       family: &F, kind: BenchKind, workload: &Workload, prefix: &str, optimiser: Optimiser,
       metrics: &[Metric], queries_per_build: u32, bench_args: &BenchArgs,
   ) -> anyhow::Result<Vec<BenchReport>> {
   ```
   and the `reports.push(BenchReport::new(BenchKind::Run, &metadata, axes, criterion_groups));` near the end of the loop becomes `BenchReport::new(kind, …)`. (`BenchKind` is `Copy`.) `kind` is the report's `kind` field: `Run` for `bench run`, `Join` for `bench join` (Task 4); the group naming is the same for both.
2. Delete the whole `let queries: Vec<&kermit_bench::QueryDefinition> = match query_filter { … };` block and the `let cached_paths = kermit_bench::cache::ensure_cached(…)…?;` statement. Replace the relation load with
   ```rust
   let relations: Vec<F::Rel> = workload
       .relation_paths
       .iter()
       .map(|p| family.load(p))
       .collect::<Result<_, _>>()?;
   ```
3. `Vec::with_capacity(queries.len())` → `Vec::with_capacity(workload.queries.len())`.
4. The loop header `for query_def in &queries {` → `for query_def in &workload.queries {`, and delete the three-line `let join_query: JoinQuery = query_def.query.trim().parse()…?;` (the query is already parsed). Where the body used `join_query`, use `query_def.query` — it is cloned in the `iter_batched` setup closures (`|| query_def.query.clone()`), so no ownership change.
5. Every `benchmark.name` → `workload.name` (metadata line, `group_name` format, `axes` map). `query_def.name` stays as is (`NamedQuery` has the same field).
6. `let prefix = bench_args.name.as_deref().unwrap_or(DEFAULT_RUN_GROUP);` — delete; `prefix` is now the parameter.
7. Update the doc comment's first paragraph to: "Runs every query of `workload` on one execution cell and returns one report per query. `prefix` is the first segment of the Criterion group (`{prefix}/{workload}/{query}/{ds}/{algo}`), resolved by the caller from `--name` or the subcommand's default."
8. `dispatch_run_bench` signature becomes
   ```rust
   pub(crate) fn dispatch_run_bench(
       cell: Execution, kind: BenchKind, workload: &Workload, prefix: &str, optimiser: Optimiser,
       metrics: &[Metric], queries_per_build: u32, bench_args: &BenchArgs,
   ) -> anyhow::Result<Vec<BenchReport>> {
   ```
   and each of its three `run_benchmark(...)` calls passes `kind, workload, prefix, optimiser, metrics, queries_per_build, bench_args` in that order after the family (drop `query_filter`). Add `#[allow(clippy::too_many_arguments)]` on both functions if clippy asks (the threshold is 7).
9. `resolve_sweep` becomes `pub(crate) fn resolve_sweep(…)`, body unchanged.

Delete the three functions from `main.rs`. In `bench/mod.rs` add `pub mod run;` and remove the `#[allow(dead_code)]` line and its comment above `pub mod workload;`.

- [ ] **Step 2: Rewire `run_bench_run_command`**

In `main.rs` the handler's loop becomes:

```rust
    let prefix = bench_args.name.as_deref().unwrap_or(DEFAULT_RUN_GROUP);
    // Opened before the loop so every finished cell is on disk before the
    // next one starts; a crash mid-sweep keeps the completed cells.
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Run)?;
    for benchmark in &materialized {
        let workload = Workload::from_definition(benchmark, &workspace_root(), query.as_deref())?;
        for &cell in &cells {
            let cell_reports = dispatch_run_bench(
                cell,
                BenchKind::Run,
                &workload,
                prefix,
                optimiser,
                &metrics,
                queries_per_build,
                bench_args,
            )
            .with_context(|| {
                format!(
                    "bench run failed on benchmark '{}' cell {:?}; partial report retained at {}",
                    benchmark.name,
                    cell,
                    sink.path().display()
                )
            })?;
            sink.push(cell_reports)?;
        }
    }
    sink.finish()?;
```

Note the workload is built once per benchmark, outside the cell loop, so `ensure_cached` and query parsing happen once per benchmark rather than once per cell (today they run per cell; the observable behaviour — same paths, same queries — is identical, and the `z-bad` CLI test still fails at `ensure_cached` before any cell of that benchmark runs, with `a-ok`'s reports already pushed).

Update `main.rs` imports: `bench::{dispatch_ds_bench, dispatch_run_bench, resolve_sweep, Metric, Workload}`; remove `Sweep`, `TrieLftj`, `HashHtj`, `ExecutionFamily`, `JoinQuery` if the compiler reports them unused (`JoinQuery` is still used by `head_column_names` / `parse_query` — keep what the compiler needs).

- [ ] **Step 3: Verify nothing changed for `bench run`**

```bash
CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test -p kermit
```
Expected: clean; all pass, in particular `cli_bench_run_sweep` (4 tests, including `partial_sweep_keeps_reports_of_finished_benchmarks`), `cli_end_to_end_metric`, `cli_optimiser_choice`, `cli_hash_trie_layout_pruning`. `grep -n 'fn run_benchmark\|fn dispatch_run_bench\|fn resolve_sweep' kermit/src/main.rs` must be empty. Also confirm the Criterion directory names are untouched: `ls target/criterion/ | head` still shows `run_triangle_…`-style entries from earlier runs and a fresh `CARGO_BUILD_JOBS=2 cargo run -q -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 -m space run triangle -i tree-trie -a leapfrog-triejoin` reuses `target/criterion/run/triangle/triangle/TreeTrie/LeapfrogTriejoin/` (check the `base/` subdir exists afterwards, meaning Criterion matched a previous run).

- [ ] **Step 4: fmt inside `nix develop`, commit**

```bash
git add kermit/src/bench kermit/src/main.rs
git commit -m "refactor(kermit): run bench run through bench::run on a Workload"
```

---

### Task 4: `bench join` through the generic runner

**Files:**
- Modify: `kermit/src/main.rs` (`BenchSubcommand::Join`, `run_bench_join`, `load_query_runner`, `run_join`)
- Modify: `kermit/tests/cli_join_tests.rs` (two tests)
- Modify: `kermit/tests/common/cli.rs` (add `bench_join`)
- Create: `kermit/tests/cli_bench_join_axes.rs`

- [ ] **Step 1: Write the failing CLI test**

Add to `kermit/tests/common/cli.rs`, after `bench_run`:

```rust
/// Runs `bench join` over the `first.csv` / `second.csv` fixtures with
/// `intersect_query.dl`, appending `extra_args` after the structure and
/// algorithm. Returns the raw process output and the report file.
pub fn bench_join(indexstructure: &str, algorithm: &str, extra_args: &[&str]) -> (Output, NamedTempFile) {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let report = NamedTempFile::new().expect("failed to create temp report file");
    let mut cmd = Command::new(kermit_bin());
    cmd.arg("bench")
        .arg("--sample-size")
        .arg("10")
        .arg("--measurement-time")
        .arg("1")
        .arg("--warm-up-time")
        .arg("1")
        .arg("--report-json")
        .arg(report.path())
        .arg("join")
        .arg("--relations")
        .arg(fixtures.join("first.csv"))
        .arg(fixtures.join("second.csv"))
        .arg("--query")
        .arg(fixtures.join("intersect_query.dl"))
        .arg("--indexstructure")
        .arg(indexstructure)
        .arg("--algorithm")
        .arg(algorithm);
    for arg in extra_args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to execute kermit binary");
    (output, report)
}
```

Create `kermit/tests/cli_bench_join_axes.rs`:

```rust
//! `bench join` goes through the same generic runner as `bench run`, so an
//! ad-hoc join reports the full metric set and the structure's
//! optimization axes, under `bench run`'s Criterion naming with the
//! `adhoc/<query-stem>` identity.

mod common;

use common::cli::{bench_join, reports_of};

#[test]
fn cli_bench_join_hash_trie_reports_axes_and_all_default_metrics() {
    let (output, report) = bench_join("hash-trie", "hash-triejoin", &[
        "--ds-layout-hasher",
        "fxhash",
        "--ds-config",
        "load-factor=0.5",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reports = reports_of(&report);
    assert_eq!(reports.len(), 1, "one query, one report");
    let r = &reports[0];
    assert_eq!(r["kind"], "join");
    let axes = &r["axes"];
    assert_eq!(axes["benchmark"], "adhoc");
    assert_eq!(axes["query"], "intersect_query");
    assert_eq!(axes["data_structure"], "HashTrie");
    assert_eq!(axes["algorithm"], "HashTriejoin");
    assert_eq!(axes["optimiser"], "lexicographic");
    assert_eq!(axes["ds_layout_hasher"], "fxhash");
    assert_eq!(axes["ds_layout_pruning"], "off");
    assert_eq!(axes["ds_config_load_factor"], 0.5);
    assert!(axes["tuples"].as_u64().unwrap() > 0);
    assert!(axes.get("relations").is_none(), "the old count axis is gone");

    let groups = r["criterion_groups"].as_array().unwrap();
    let group = "join/adhoc/intersect_query/HashTrie/HashTriejoin";
    assert!(groups.iter().all(|g| g["group"] == group), "{groups:?}");
    let mut functions: Vec<&str> = groups.iter().map(|g| g["function"].as_str().unwrap()).collect();
    functions.sort_unstable();
    assert_eq!(functions, ["insertion", "iteration", "space/first", "space/second"]);
}

#[test]
fn cli_bench_join_name_is_a_prefix() {
    let (output, report) = bench_join("tree-trie", "leapfrog-triejoin", &[
        "-m",
        "space",
        "--name",
        "mine",
    ]);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let groups = reports_of(&report)[0]["criterion_groups"].clone();
    assert_eq!(groups[0]["group"], "mine/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin");
}

#[test]
fn cli_bench_join_rejects_ds_config_on_tree_trie() {
    let (output, _) = bench_join("tree-trie", "leapfrog-triejoin", &["--ds-config", "load-factor=0.5"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--ds-config"), "{stderr}");
}
```

Verified: the `ds_layout_pruning` default label is `"off"` (`PruningChoice` in `kermit/src/options.rs`) and `ds_config_load_factor` serialises as a JSON number (`serde_json::Value::from(0.5_f64)`, pinned in `kermit-ds/src/ds/hash_trie/implementation.rs`'s tests), so `assert_eq!(axes["ds_config_load_factor"], 0.5)` is correct.

`--name` is a `bench`-level flag and must precede `join`, but `bench_join` appends `extra_args` after `join`. So the second test does NOT use the helper; replace its body with an inline command:

```rust
#[test]
fn cli_bench_join_name_is_a_prefix() {
    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let report = tempfile::NamedTempFile::new().unwrap();
    let output = std::process::Command::new(common::cli::kermit_bin())
        .args(["bench", "--sample-size", "10", "--measurement-time", "1", "--warm-up-time", "1"])
        .arg("--name")
        .arg("mine")
        .arg("--report-json")
        .arg(report.path())
        .arg("join")
        .arg("--relations")
        .arg(fixtures.join("first.csv"))
        .arg(fixtures.join("second.csv"))
        .arg("--query")
        .arg(fixtures.join("intersect_query.dl"))
        .args(["-i", "tree-trie", "-a", "leapfrog-triejoin", "-m", "space"])
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let groups = reports_of(&report)[0]["criterion_groups"].clone();
    assert_eq!(groups[0]["group"], "mine/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin");
}
```

(`kermit_bin` is `pub` in `common/cli.rs`; add it to the test file's `use`.)

- [ ] **Step 2: Confirm the test fails**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_join_axes`
Expected: the first test fails on `benchmark` (missing axis) or on the `--ds-config` flag being rejected by clap ("unexpected argument") — either way FAIL.

- [ ] **Step 3: Extend the clap surface**

In `main.rs`, `BenchSubcommand::Join` becomes:

```rust
    /// Benchmark a join query over relation files.
    ///
    /// Runs through the same runner as `bench run`, with the workload named
    /// `adhoc` and the query named by the query file's stem, so the Criterion
    /// group is `{--name|join}/adhoc/{stem}/{ds}/{algo}`. No `-i all` /
    /// `-a all` sweep here; use `bench run` for sweeps.
    Join {
        #[command(flatten)]
        query_args: QueryArgs,

        /// Output file for one run's results (optional)
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,

        /// Metrics to benchmark (`end-to-end` is opt-in, not in the default
        /// set)
        #[arg(
            short,
            long,
            value_enum,
            num_args = 1..,
            default_values_t = vec![Metric::Insertion, Metric::Iteration, Metric::Space]
        )]
        metrics: Vec<Metric>,

        /// Query executions per database build in the `end-to-end` metric's
        /// timed body (T = build + K × query). Ignored by other metrics.
        #[arg(long, value_name = "K", default_value = "1", value_parser = clap::value_parser!(u32).range(1..))]
        queries_per_build: u32,

        #[command(flatten)]
        config: ConfigChoices,
    },
```

and the `main()` match arm passes the new fields: `BenchSubcommand::Join { query_args, output, metrics, queries_per_build, config } => run_bench_join(&bench_args, query_args, output, &metrics, queries_per_build, config)?`.

- [ ] **Step 4: Thread the config through the one-shot runner**

`load_query_runner` currently hardcodes `HashTrieConfig::default()`. Change its signature to `fn load_query_runner(args: &QueryArgs, config: HashTrieConfig) -> anyhow::Result<JoinRunner>` and use `config` in the `Execution::for_pair(...)` call. Update its doc comment: replace the paragraph starting "`kermit join` deliberately carries no `--ds-config`" with "`kermit join` deliberately carries no `--ds-config` and passes [`HashTrieConfig::default()`]: the one Config value trades space against probe length and cannot change a query's answers. `bench join --output` passes its resolved config so the CSV comes from the same build the measurements use." `run_join` calls `load_query_runner(&query_args, HashTrieConfig::default())`.

- [ ] **Step 5: Rewrite the handler**

Add near `IndexStructureSelector`:

```rust
impl IndexStructureSelector {
    /// The selector naming exactly `ds`, so the `--ds-layout-*` /
    /// `--ds-config` validators (which take a selector) apply to a
    /// concrete structure.
    fn of(ds: IndexStructure) -> Self {
        match ds {
            | IndexStructure::ColumnTrie => Self::ColumnTrie,
            | IndexStructure::HashTrie => Self::HashTrie,
            | IndexStructure::TreeTrie => Self::TreeTrie,
        }
    }
}
```

(merge into the existing `impl IndexStructureSelector` block). Replace `run_bench_join` entirely with:

```rust
/// Handler for `bench join`: an ad-hoc workload over the given relation
/// files, run through the same generic runner as `bench run` under the
/// `adhoc/{query-stem}` identity.
fn run_bench_join(
    bench_args: &BenchArgs, query_args: QueryArgs, output: Option<PathBuf>, metrics: &[Metric],
    queries_per_build: u32, config: ConfigChoices,
) -> anyhow::Result<()> {
    let selector = IndexStructureSelector::of(query_args.indexstructure);
    validate_layout_choices(selector, &query_args.layout)?;
    validate_config_choices(selector, &config)?;
    let hash_trie_config = config.hash_trie_config_resolved()?;
    let cell = Execution::for_pair(
        query_args.indexstructure,
        query_args.algorithm,
        query_args.layout.hash_trie_hasher_resolved(),
        query_args.layout.hash_trie_pruning_resolved(),
        hash_trie_config,
    )
    .ok_or_else(|| {
        anyhow::anyhow!(
            "incompatible selection: {:?} cannot run under {:?}",
            query_args.indexstructure,
            query_args.algorithm
        )
    })?;

    if let Some(path) = &output {
        let join_query = parse_query(&query_args)?;
        let join = load_query_runner(&query_args, hash_trie_config)?;
        let header = head_column_names(&join_query);
        let tuples = join(join_query);
        let writer = BufWriter::new(fs::File::create(path)?);
        write_tuples(writer, &header, &tuples)?;
    }

    let workload = Workload::adhoc(query_args.relations.clone(), &query_args.query)?;
    let prefix = bench_args.name.as_deref().unwrap_or(DEFAULT_JOIN_GROUP);
    let mut sink = ReportSink::open(bench_args.report_json.as_deref(), BenchKind::Join)?;
    let reports = dispatch_run_bench(
        cell,
        BenchKind::Join,
        &workload,
        prefix,
        query_args.optimiser,
        metrics,
        queries_per_build,
        bench_args,
    )?;
    sink.push(reports)?;
    sink.finish()?;
    Ok(())
}
```

`BenchKind::Join` flows through `dispatch_run_bench` (Task 3) into every report, so the file keeps `kind: "join"`. The `DEFAULT_JOIN_GROUP` doc comment becomes "Default Criterion group *prefix* for `bench join` (`{prefix}/adhoc/{query}/{ds}/{algo}`)."

Delete the now-unused `parse_query` only if the compiler says it is unused (it is still used by `run_join`).

- [ ] **Step 6: Re-point the two existing `bench join` tests**

In `kermit/tests/cli_join_tests.rs`:

- `cli_bench_runs_criterion`: change the stderr assertion `stderr.contains("--- bench metadata ---")` to `stderr.contains("--- bench run metadata ---")`, and add `assert!(stderr.contains("benchmark:") && stderr.contains("adhoc"), "{stderr}");`. Change the stdout assertion `stdout.contains("test-intersect")` to `stdout.contains("test-intersect/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin")`.
- `cli_bench_default_name`: change `stdout.contains("join/")` to `stdout.contains("join/adhoc/intersect_query/TreeTrie/LeapfrogTriejoin/")`.

Both tests pass `--sample-size 10 --measurement-time 1` but not `--warm-up-time`; the default 3 s warm-up now applies to three functions instead of one. Add `"--warm-up-time", "1"` to both tests' `bench_args` so the suite does not slow down.

- [ ] **Step 7: Run everything**

```bash
CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test -p kermit
```
Expected: clean; all pass including the 3 new tests in `cli_bench_join_axes` and the 19 in `cli_join_tests`. Also `grep -n 'JoinRunner' kermit/src/main.rs` shows it used only by `run_join` / `load_query_runner` / `build_join_runner`, and `grep -n '"relations"' kermit/src/main.rs` is empty.

- [ ] **Step 8: fmt inside `nix develop`, commit**

```bash
git add kermit/src/main.rs kermit/src/bench kermit/tests/cli_join_tests.rs kermit/tests/common/cli.rs kermit/tests/cli_bench_join_axes.rs
git commit -m "feat(kermit): run bench join through the generic runner with metrics, axes and --ds-config"
```

---

### Task 5: Docs, spec status, full gate

**Files:**
- Modify: `USAGE.md`, `BENCHMARKING.md`, `CLAUDE.md`, `docs/specs/benchmarking-architecture.md`, `docs/specs/bench-report-schema.md`, `docs/specs/2026-09-09-runner-consolidation-design.md`

- [ ] **Step 1: `USAGE.md`**

In the "Benchmark a join (`bench join`)" section (around line 117), after the example command add:

> `bench join` runs through the same runner as `bench run`: it records the `insertion`, `iteration` and `space/<relation>` metrics by default (`-m` selects, `end-to-end` is opt-in with `--queries-per-build K`), accepts `--ds-config` alongside the `--ds-layout-*` flags, and writes its Criterion output under `{--name|join}/adhoc/{query-file-stem}/{ds}/{algo}`. The report's `benchmark` axis is `adhoc` and `query` is the query file's stem.

In the `--name` semantics paragraph (around line 113), change "for `bench join` / `bench ds` it is the full group name" to "for `bench ds` it is the full group name (default `ds`); for `bench join` and `bench run` it is a prefix on the auto-generated identity (defaults `join` / `run`)".

- [ ] **Step 2: `BENCHMARKING.md`**

Line ~40: replace "(`bench join` is time-only and has no `--metrics` flag — it times the full query each iteration.)" with "(`bench join` accepts the same `--metrics` as `bench run`; it is the ad-hoc form of the same measurement.)"

- [ ] **Step 3: `CLAUDE.md`**

- Gotcha "bench `--name` semantics": change to "For `bench ds`, `--name` is the full Criterion group name (default `ds`). For `bench run` and `bench join` it is a *prefix* on the auto-generated `{workload}/{query}/{ds}/{algo}` identity (defaults `run` / `join`; `bench join`'s workload is `adhoc` and its query is the query file's stem), so workload identity stays in `target/criterion/{group}/`."
- Gotcha "`bench run` sweeps are cells, not pairs": replace "One generic `run_benchmark<F: ExecutionFamily>` in `main.rs` serves both trait families" with "One generic `run_benchmark<F: ExecutionFamily>` in `kermit/src/bench/run.rs` serves both trait families and both `bench run` and `bench join`, fed by a `bench::Workload` (`bench/workload.rs`: `from_definition` for YAML benchmarks, `adhoc` for `bench join`)"; and "one generic `run_ds_bench<F: RelationFamily>` serves both" → "one generic `run_ds_bench<F: RelationFamily>` in `kermit/src/bench/ds.rs` serves both".
- Workspace Architecture, `kermit` line: after "All Criterion execution lives here" add " (in `src/bench/`)".
- "Extending the System" → "Adding a new index structure" step 6 and "Adding a new join algorithm" step 4: `dispatch_run_bench` / `dispatch_ds_bench` "in `main.rs`" → "in `kermit/src/bench/{run,ds}.rs`".

- [ ] **Step 4: `docs/specs/benchmarking-architecture.md`**

Replace the `## kermit bench join` section's **Flow** list with:

1. Validate the layout/config flags for the concrete structure and resolve the `(structure, algorithm)` pair to its `Execution` cell.
2. If `--output` is set, run the join once through `load_query_runner` (built with the resolved config) and write CSV with a header row.
3. Build `bench::Workload::adhoc` (name `adhoc`, one query named by the file stem) and run it through `bench::run::dispatch_run_bench` — the same runner as `bench run` — with prefix `--name` or `join`.

and add `--metrics`, `--queries-per-build`, `--ds-config` to its **Arguments** line. In the "Common arguments" table, the `--name` row: "Criterion group name (`bench ds`) or prefix (`bench join`, `bench run`)". Update the `run_benchmark` mention under `bench run` to say it lives in `kermit/src/bench/run.rs` and takes a `Workload`.

- [ ] **Step 5: `docs/specs/bench-report-schema.md`**

In the conventional-keys table: `benchmark` and `query` rows now list `join` among their subcommands with the note "`bench join`: `adhoc` / the query file's stem"; delete the `relations` row (or mark it "removed 2026-09-09; `bench join` now reports `tuples` like `bench run`" if the table keeps history); `tuples` row adds `join`. Add a line under the version note: "Schema version stays `2`: `bench join` gained keys it lacked and lost the redundant `relations` count; no key changed name or type."

Then in `python/kermit-lab/kermit_lab/frame.py`, check `_AXIS_INT_KEYS` for `"relations"`: if present, leave it (a missing key becomes NA; removing it would break loading old reports) and add nothing.

- [ ] **Step 6: Spec status and gate**

Set `**Status:** Implemented 2026-09-09` in `docs/specs/2026-09-09-runner-consolidation-design.md` and, under "## 2. Module layout", add one line: "Implementation note: `run_benchmark` / `dispatch_run_bench` also take a `BenchKind` so `bench join` reports keep `kind: "join"`; `load_query_runner` takes the resolved `HashTrieConfig` so `bench join --output` writes CSV from the same build it measures."

Run the full gate inside `nix develop`:

```bash
nix develop --command bash -c 'cargo fmt --all --check && CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets && RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps && CARGO_BUILD_JOBS=2 cargo test'
```
Expected: all clean, no test failures.

Manual checks from the spec:

```bash
CARGO_BUILD_JOBS=2 cargo run -q -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 --report-json /tmp/claude-1000/join.json join --relations kermit/tests/fixtures/first.csv kermit/tests/fixtures/second.csv --query kermit/tests/fixtures/intersect_query.dl -i hash-trie -a hash-triejoin --ds-config load-factor=0.5 -m space
python3 -c "import json; r=json.load(open('/tmp/claude-1000/join.json'))[0]; print(r['kind'], r['axes']); print([g['group']+'::'+g['function'] for g in r['criterion_groups']])"
```
Expected: `join {…'benchmark': 'adhoc', 'query': 'intersect_query', 'ds_config_load_factor': 0.5, …}` and groups `join/adhoc/intersect_query/HashTrie/HashTriejoin::space/first` and `::space/second`.

- [ ] **Step 7: Commit**

```bash
git add USAGE.md BENCHMARKING.md CLAUDE.md docs/specs/benchmarking-architecture.md docs/specs/bench-report-schema.md docs/specs/2026-09-09-runner-consolidation-design.md
git commit -m "docs: describe bench join's generic-runner surface and the bench/ module"
```
