# Answer Verification, Rust–Python Contract, Download Integrity — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `bench run --verify` checks each query's result count against an `expected` value carried in the YAML (C); a compile-time pin, a `verified` DataFrame column, a real-binary contract test and a CI job tie the Rust report schema to `kermit-lab` (D); relations can declare a `sha256` that is checked on download and by `bench fetch` (E).

**Architecture:** Spec `docs/specs/2026-09-09-answer-verification-design.md`. C threads `Option<u64>` from `QueryDefinition` → generator YAML emission / hand-written YAML → `Workload::NamedQuery` → `run_benchmark`, which runs the query once before Criterion when `RunSettings.verify` is set. D adds a `include_str!` pin test in `bench_report.rs`, `_AXIS_BOOL_KEYS` in `frame.py`, `tests/test_contract.py` gated on `KERMIT_BIN`, and a `python` CI job. E adds `RelationSource.sha256`, streaming `sha256_hex`, a `BenchError::Integrity` variant, digest checks in `download_file`, and `verify_integrity` called by `bench fetch`.

**Tech Stack:** Rust nightly, serde/serde_yaml, sha2 0.11 (already a `kermit-bench` dep), clap, Criterion; Python 3.10+ with pandas and pytest under `uv`; GitHub Actions.

**Conventions:** every match arm starts with `| `; one `use { ... };` block per file; backtick identifiers in doc comments (clippy `doc_markdown` under `-Dwarnings`); comments wrap at 80 columns. Run cargo in the foreground with `CARGO_BUILD_JOBS=2`. Run `cargo fmt` ONLY as `nix develop --command cargo fmt --all` (stable rustfmt rewrites the tree; if `git status` shows unexpected files after fmt, `git checkout -- .` and retry inside `nix develop`); `nix develop --command cargo fmt --all --check` must be clean before every commit. Commit messages are conventional commits ending with the session's attribution trailer; never amend.

**Order:** Tasks 1→9 are sequential. Tasks 1–3 are C, 4–6 are D, 7–8 are E, 9 is docs + gate.

---

## File map

| File | Task | Change |
| --- | --- | --- |
| `kermit-bench/src/definition.rs` | 1, 7 | `QueryDefinition.expected`; `RelationSource.sha256` + validation; `hex_digest` → `pub(crate)` |
| `kermit-bench/src/error.rs` | 7 | `BenchError::Integrity` |
| `kermit-bench/src/cache.rs` | 7 | `sha256_hex`, `check_digest`, digest check in `download_file`, `verify_integrity` |
| `kermit-rdf/src/generator.rs` | 2 | `TranslatedQuery`; drop `write_expected` hook and stage F |
| `kermit-rdf/src/yaml_emit.rs` | 2 | write `expected` |
| `kermit-rdf/src/pipeline.rs`, `kermit-rdf/src/lubm/pipeline.rs` | 2 | new `translate_queries` return type; drop `write_expected` |
| `kermit-rdf/src/expected.rs`, `kermit-rdf/src/lib.rs` | 2 | module removed |
| `kermit-rdf/src/lubm/queries.rs` | 2 | `lubm_reference_applies` |
| `kermit-rdf/tests/e2e_watdiv.rs`, `kermit-rdf/src/lubm/README.md` | 2 | no `expected/` dir |
| `kermit/src/materialize.rs`, `kermit/src/main.rs` | 2, 3, 8 | LUBM gate; `--verify`; `bench fetch` verifies |
| `kermit/src/bench/workload.rs`, `kermit/src/bench/run.rs` | 3 | `expected` on `NamedQuery`; `RunSettings.verify`; verification step |
| `benchmarks/triangle.yml` | 3, 8 | `expected: 4`; `sha256` |
| `kermit/tests/cli_bench_run_verify.rs` | 3 | new CLI tests |
| `kermit/src/bench_report.rs` | 4 | schema-version pin test |
| `python/kermit-lab/kermit_lab/frame.py`, `tests/conftest.py`, `tests/test_frame.py` | 5 | `verified` column |
| `python/kermit-lab/tests/test_contract.py`, `python/kermit-lab/README.md` | 5 | contract test |
| `.github/workflows/pr.yml`, `.github/workflows/build.yml` | 6 | `python` job |
| `kermit/tests/cli_bench_fetch_integrity.rs` | 8 | new CLI tests |
| docs (see Task 9) | 9 | |

---

### Task 1 (C): `QueryDefinition.expected`

**Files:**
- Modify: `kermit-bench/src/definition.rs` (struct at ~line 203; tests module at the bottom, which has `fn make_query(name, query)` at ~line 503)

- [ ] **Step 1: Failing tests**

Append inside `mod tests` in `kermit-bench/src/definition.rs`:

```rust
    #[test]
    fn query_expected_round_trips_through_yaml() {
        let mut q = make_query("t", "Q(X) :- r(X).");
        q.expected = Some(7);
        let yaml = serde_yaml::to_string(&q).unwrap();
        assert!(yaml.contains("expected: 7"), "{yaml}");
        let back: QueryDefinition = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.expected, Some(7));
    }

    #[test]
    fn query_without_expected_serialises_without_the_key() {
        let q = make_query("t", "Q(X) :- r(X).");
        let yaml = serde_yaml::to_string(&q).unwrap();
        assert!(!yaml.contains("expected"), "{yaml}");
    }

    #[test]
    fn legacy_query_yaml_without_expected_deserialises_to_none() {
        let back: QueryDefinition =
            serde_yaml::from_str("name: t\ndescription: d\nquery: 'Q(X) :- r(X).'\n").unwrap();
        assert_eq!(back.expected, None);
    }
```

If `make_query` builds `QueryDefinition` with a struct literal, it will fail to compile until Step 3 adds the field; that is the expected failure.

- [ ] **Step 2: Verify failure**

Run: `CARGO_BUILD_JOBS=2 cargo test -p kermit-bench query_expected` — compile error: no field `expected`.

- [ ] **Step 3: Implement**

In `pub struct QueryDefinition` add after `query`:

```rust
    /// Expected number of result tuples, when known. Read by `bench run
    /// --verify`; absent means "unknown", not "zero".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<u64>,
```

Add `expected: None,` to every `QueryDefinition { … }` literal the compiler reports (in `make_query` and any other test helper in this file). No validation change.

- [ ] **Step 4: Verify pass**

`CARGO_BUILD_JOBS=2 cargo test -p kermit-bench` — all pass (3 new). `CARGO_BUILD_JOBS=2 cargo clippy -p kermit-bench --all-targets -- -D warnings` clean.

Then build the workspace to find every other `QueryDefinition { … }` literal: `CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets`. Add `expected: None,` wherever it fails (`kermit-rdf/src/yaml_emit.rs`, `kermit/src/materialize.rs` tests, `kermit/src/bench/workload.rs` tests, `kermit/tests/*.rs` if any). Re-run the check until clean.

- [ ] **Step 5: fmt, commit**

```bash
git add -A kermit-bench kermit-rdf kermit
git commit -m "feat(kermit-bench): optional expected result cardinality on query definitions"
```

---

### Task 2 (C): generators write `expected`; drop the sidecars; LUBM(1, 0) gate

**Files:**
- Modify: `kermit-rdf/src/generator.rs`, `yaml_emit.rs`, `pipeline.rs`, `lubm/pipeline.rs`, `lubm/queries.rs`, `lib.rs`, `lubm/README.md`
- Delete: `kermit-rdf/src/expected.rs`
- Modify: `kermit-rdf/tests/e2e_watdiv.rs`
- Modify: `kermit/src/materialize.rs` (~line 339), `kermit/src/main.rs` (`run_gen_lubm`, ~line 1004)

- [ ] **Step 1: Failing tests**

`kermit-rdf/src/lubm/queries.rs`, inside its `mod tests`:

```rust
    #[test]
    fn reference_cardinalities_apply_only_to_lubm_1_0() {
        assert!(lubm_reference_applies(1, 0, 0));
        assert!(!lubm_reference_applies(2, 0, 0));
        assert!(!lubm_reference_applies(1, 5, 0));
        assert!(!lubm_reference_applies(1, 0, 3));
    }
```

`kermit-rdf/src/yaml_emit.rs`: find its `mod tests` (add one if absent, with `use super::*;`) and add:

```rust
    #[test]
    fn expected_cardinality_is_written_only_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let preds = vec!["r".to_string()];
        let inputs = YamlInputs {
            name: "t",
            description: "d",
            queries: vec![
                TranslatedQuery {
                    name: "with".to_string(),
                    datalog: "Q_with(X) :- r(X, X).".to_string(),
                    expected: Some(3),
                },
                TranslatedQuery {
                    name: "without".to_string(),
                    datalog: "Q_without(X) :- r(X, X).".to_string(),
                    expected: None,
                },
            ],
            all_predicates: &preds,
            base_url: "file:///x",
        };
        write_benchmark_yaml(&inputs, dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("benchmark.yml")).unwrap();
        assert_eq!(text.matches("expected: 3").count(), 1, "{text}");
        let def: kermit_bench::BenchmarkDefinition = serde_yaml::from_str(&text).unwrap();
        assert_eq!(def.queries[0].expected, Some(3));
        assert_eq!(def.queries[1].expected, None);
    }
```

(`tempfile` and `serde_yaml` are already dev/regular deps of `kermit-rdf`; check `kermit-rdf/Cargo.toml` and add `serde_yaml` under `[dev-dependencies]` if it is not a regular dep.)

- [ ] **Step 2: Verify failure**

`CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf --lib reference_cardinalities expected_cardinality_is_written` — compile errors: `lubm_reference_applies`, `TranslatedQuery` not found.

- [ ] **Step 3: `TranslatedQuery` and the trait**

In `kermit-rdf/src/generator.rs`, above `pub trait Generator`:

```rust
/// One translated query for `benchmark.yml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatedQuery {
    /// Query name (`q1`, `tiny_q0002`, …); the YAML `name`.
    pub name: String,
    /// The Datalog rule text.
    pub datalog: String,
    /// Expected result cardinality when the generator knows it (LUBM(1, 0)
    /// from the paper's Table 3); `None` otherwise.
    pub expected: Option<u64>,
}
```

Change the trait: `translate_queries` returns `Result<Vec<TranslatedQuery>, RdfError>` (doc: "Produces the queries for `benchmark.yml`, growing `dict` for constants the data never mentioned."). Delete the `write_expected` method and its doc. In `process_artifacts` delete the three "Stage F" lines (`expected_dir` creation and the `write_expected` call) and renumber the comment `Stage G` → `Stage F`. Update the module doc's stage table (`//!` block near the top, lines ~7–23) to drop the `write_expected` row and to say `translate → [TranslatedQuery]`. The toy generator in the tests module: change its `translate_queries` to return `vec![TranslatedQuery { name: "path".into(), datalog: dl, expected: Some(3) }]`, delete its `write_expected` impl and the `use crate::expected::write_cardinality_csv`, and replace the assertion that reads `expected/path.csv` with:

```rust
        assert!(!out.join("expected").exists(), "expected/ sidecars are gone");
        assert!(yaml.contains("expected: 3"), "{yaml}");
```

- [ ] **Step 4: `yaml_emit.rs`**

`YamlInputs.queries: Vec<TranslatedQuery>` (import `crate::generator::TranslatedQuery`). `collect_used_predicates` iterates `.datalog`. The `QueryDefinition` construction becomes:

```rust
        .map(|q| QueryDefinition {
            name: q.name.clone(),
            description: format!("query {}", q.name),
            query: q.datalog.clone(),
            expected: q.expected,
        })
```

- [ ] **Step 5: the two generators**

`kermit-rdf/src/pipeline.rs` (WatDiv): `translate_queries` builds `TranslatedQuery { name: qname, datalog: dl, expected: None }` and returns `Vec<TranslatedQuery>`; delete its `write_expected` impl and the `expected::` import. `kermit-rdf/src/lubm/pipeline.rs`: `translate_queries` pushes `TranslatedQuery { name: spec.name.clone(), datalog: dl, expected: spec.expected_cardinality }`; delete `write_expected` and the `write_cardinality_csv` import; update the `LubmQuerySpec::expected_cardinality` doc (currently says "Written to expected/…") to "Written into `benchmark.yml` as the query's `expected` field."

Delete `kermit-rdf/src/expected.rs` and the `pub mod expected;` line in `lib.rs`. Keep `RdfError::Expected` (still used by `yaml_emit`) but change its doc/message if it mentions sidecars (grep `error.rs`).

- [ ] **Step 6: the LUBM gate**

`kermit-rdf/src/lubm/queries.rs`, above `lubm_query_specs`:

```rust
/// Whether the paper's LUBM(1, 0) reference cardinalities apply to a
/// generation: scale 1, seed 0, start index 0. Any other setting produces a
/// different ABox, so the counts must not be attached.
#[must_use]
pub fn lubm_reference_applies(scale: u32, seed: u64, start_index: u32) -> bool {
    scale == 1 && seed == 0 && start_index == 0
}
```

Use the real types of `scale` / `seed` / `start_index` as they appear in `LubmDriverInputs` (`grep -n 'pub seed\|pub scale\|pub start_index' kermit-rdf/src/lubm/driver.rs`) and adjust the signature to match. Then in `kermit/src/materialize.rs::run_lubm` replace `lubm_query_specs(scale == 1)` with `lubm_query_specs(kermit_rdf::lubm::queries::lubm_reference_applies(scale, seed, start_index))` using the local variable names in scope (read the function; the `GeneratorSpec::Lubm` destructuring names them), and the same in `kermit/src/main.rs::run_gen_lubm`, replacing its three-line comment about `expected.csv` with `// Reference cardinalities are only attached for LUBM(1, 0).`

- [ ] **Step 7: tests and docs in `kermit-rdf`**

`kermit-rdf/tests/e2e_watdiv.rs` (~lines 119–140): replace the whole `expected_dir` block (existence check, csv listing, per-csv assertions) with `assert!(!dir.path().join("expected").exists(), "expected/ sidecars are no longer written");`. Grep `kermit-rdf/tests` and `kermit/tests` for `expected/` or `"expected"` directory references and treat any other hit the same way. `kermit-rdf/src/lubm/README.md`: remove the `expected/q*.csv` line from the layout listing and change the pipeline row to `… → emit YAML (with expected cardinalities)/dict/meta`.

- [ ] **Step 8: Verify**

```bash
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps
CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf -p kermit
```
Expected: clean; all pass (the java/bwrap-gated tests skip or pass). `grep -rn 'write_expected\|expected::' kermit-rdf/src kermit/src` is empty.

- [ ] **Step 9: fmt, commit**

```bash
git add -A kermit-rdf kermit
git commit -m "feat(kermit-rdf): write expected cardinalities into benchmark.yml and drop the sidecars"
```

---

### Task 3 (C): `bench run --verify`

**Files:**
- Modify: `kermit/src/bench/workload.rs`, `kermit/src/bench/run.rs`, `kermit/src/main.rs`, `benchmarks/triangle.yml`
- Create: `kermit/tests/cli_bench_run_verify.rs`

- [ ] **Step 1: Failing CLI tests**

Create `kermit/tests/cli_bench_run_verify.rs`:

```rust
//! `bench run --verify`: each query with an `expected` cardinality is run
//! once before timing and its tuple count compared; a mismatch aborts.
//!
//! Runs against fake caches built from the committed watdiv-mini fixture
//! (`q0002` has 6 answers), so no network is needed.

use {
    std::{fs, path::PathBuf, process::Command},
    tempfile::{NamedTempFile, TempDir},
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn fixtures_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

const MINI_BENCH: &str = "watdiv-stress-mini-tiny";
const Q0002_LINE: &str = "  query: Q_tiny_q0002(V0, V1) :- parentcountry(V0, V1).";

fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_bench_run_verify test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// Copies the mini fixture into `<cache>/kermit/benchmarks/<name>/`,
/// renaming the YAML's `name:` and, when `expected` is given, adding
/// `expected: <n>` to query `q0002`.
fn add_benchmark(cache: &TempDir, name: &str, expected: Option<u64>) {
    let dir = cache.path().join("kermit/benchmarks").join(name);
    fs::create_dir_all(&dir).unwrap();
    let artifacts = fixtures_dir().join("watdiv-mini/artifacts");
    let mut yaml = fs::read_to_string(artifacts.join(format!("{MINI_BENCH}.yml"))).unwrap();
    yaml = yaml.replacen(&format!("name: {MINI_BENCH}"), &format!("name: {name}"), 1);
    assert!(yaml.contains(Q0002_LINE), "fixture drifted: {yaml}");
    if let Some(n) = expected {
        yaml = yaml.replacen(Q0002_LINE, &format!("{Q0002_LINE}\n  expected: {n}"), 1);
    }
    fs::write(dir.join("benchmark.yml"), yaml).unwrap();
    fs::write(dir.join("meta.json"), "{}").unwrap();
    for rel in ["eligibleregion", "includes", "parentcountry"] {
        fs::copy(
            artifacts.join(format!("{rel}.parquet")),
            dir.join(format!("{rel}.parquet")),
        )
        .unwrap();
    }
}

/// `bench run --all -q q0002 -i tree-trie -a leapfrog-triejoin -m iteration`
/// with the given extra args, against `cache`, from an empty workspace.
fn run(cache: &TempDir, extra: &[&str]) -> (std::process::Output, Vec<serde_json::Value>) {
    let workspace = tempfile::tempdir().unwrap();
    let report = NamedTempFile::new().unwrap();
    let output = Command::new(kermit_bin())
        .env("XDG_CACHE_HOME", cache.path())
        .env("KERMIT_WORKSPACE", workspace.path())
        .args(["bench", "--sample-size", "10", "--measurement-time", "1", "--warm-up-time", "1"])
        .arg("--report-json")
        .arg(report.path())
        .args(["run", "--all", "-q", "q0002", "-i", "tree-trie", "-a", "leapfrog-triejoin"])
        .args(["--metrics", "iteration"])
        .args(extra)
        .output()
        .unwrap();
    let text = fs::read_to_string(report.path()).unwrap();
    let reports = if text.trim().is_empty() { vec![] } else { serde_json::from_str(&text).unwrap() };
    (output, reports)
}

#[test]
fn verify_passes_and_stamps_the_axis_when_the_count_matches() {
    if skip_unsupported() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    add_benchmark(&cache, "a-ok", Some(6));
    let (output, reports) = run(&cache, &["--verify"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["axes"]["verified"], true);
    assert!(stderr.contains("verified:"), "metadata block should show verified; {stderr}");
}

#[test]
fn verify_aborts_on_a_wrong_count_and_keeps_earlier_reports() {
    if skip_unsupported() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    add_benchmark(&cache, "a-ok", Some(6));
    add_benchmark(&cache, "z-bad", Some(5));
    let (output, reports) = run(&cache, &["--verify"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "z-bad must fail; stderr: {stderr}");
    assert!(stderr.contains("verification failed"), "{stderr}");
    assert!(stderr.contains("returned 6 tuples, expected 5"), "{stderr}");
    assert!(stderr.contains("partial report retained at"), "{stderr}");
    assert_eq!(reports.len(), 1, "a-ok's report survives: {reports:?}");
    assert_eq!(reports[0]["axes"]["benchmark"], "a-ok");
}

#[test]
fn verify_notes_queries_without_an_expected_count() {
    if skip_unsupported() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    add_benchmark(&cache, "a-ok", None);
    let (output, reports) = run(&cache, &["--verify"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("not verified"), "{stderr}");
    assert!(reports[0]["axes"].get("verified").is_none());
}

#[test]
fn without_the_flag_nothing_is_verified() {
    if skip_unsupported() {
        return;
    }
    let cache = tempfile::tempdir().unwrap();
    add_benchmark(&cache, "a-ok", Some(5)); // wrong on purpose: must not matter
    let (output, reports) = run(&cache, &[]);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(reports[0]["axes"].get("verified").is_none());
}
```

- [ ] **Step 2: Verify failure**

`CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_run_verify` — the three `--verify` tests fail (clap: unexpected argument `--verify`); the fourth may already pass.

- [ ] **Step 3: `Workload`**

`kermit/src/bench/workload.rs`: add to `NamedQuery`:

```rust
    /// Expected result cardinality, when the definition declares one; read
    /// only under `--verify`.
    pub expected: Option<u64>,
```

`from_definition`: `expected: q.expected,`; `adhoc`: `expected: None,`. Extend the unit test `from_definition_keeps_every_query_in_order` (set `expected: Some(2)` on the `pair` query in `local_def` and assert `w.queries[0].expected == Some(2)` and `w.queries[1].expected == None`) and `adhoc_names_query_by_file_stem` (assert `w.queries[0].expected.is_none()`).

- [ ] **Step 4: `RunSettings.verify` and the runner**

`kermit/src/bench/run.rs`: add to `RunSettings`:

```rust
    /// Run each query once before timing and compare its tuple count with
    /// the workload's `expected`; a mismatch aborts.
    pub verify: bool,
```

and bind it in `run_benchmark`'s destructuring. Inside the per-query loop, directly after `let mut metadata = vec![…];` and before the `EndToEnd` metadata push, insert:

```rust
        // Correctness gate: with `--verify`, run the query once (untimed)
        // and compare the answer count before spending any measurement time
        // on it. A mismatch aborts so a wrong answer can never yield a
        // plausible timing.
        let verified = if verify {
            match query_def.expected {
                | Some(expected) => {
                    let actual = family.join(&engine, query_def.query.clone()).len() as u64;
                    if actual != expected {
                        anyhow::bail!(
                            "verification failed: benchmark '{}' query '{}' on {}/{} returned \
                             {} tuples, expected {}",
                            workload.name,
                            query_def.name,
                            ds_name,
                            algo_name,
                            actual,
                            expected
                        );
                    }
                    metadata.push(MetadataLine::new("verified", "yes"));
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

and after `axes.extend(optimization_axes.clone());`:

```rust
        if verified {
            axes.insert("verified".to_string(), serde_json::json!(true));
        }
```

- [ ] **Step 5: CLI wiring**

`kermit/src/main.rs`: in `BenchSubcommand::Run`, after `force: bool`, add:

```rust
        /// Run each query once before timing and check its result count
        /// against the benchmark's `expected` value; a mismatch aborts.
        /// Queries without `expected` are noted and skipped.
        #[arg(long)]
        verify: bool,
```

Thread `verify: bool` through the `main()` match arm and `run_bench_run_command`'s parameters (place it after `force`), and set `verify` in its `RunSettings { … }`. In `run_bench_join`'s `RunSettings` set `verify: false`.

`benchmarks/triangle.yml`: add `    expected: 4` under the `triangle` query (after `query:`).

- [ ] **Step 6: Verify**

`CARGO_BUILD_JOBS=2 cargo test -p kermit` — all pass including 4 new. `CARGO_BUILD_JOBS=2 cargo clippy -p kermit --all-targets -- -D warnings` clean. Then the real thing:

```bash
CARGO_BUILD_JOBS=2 cargo run -q -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 --report-json /tmp/claude-1000/verify.json run triangle -i all -a all -m iteration --verify
python3 -c "import json; print([(r['axes']['data_structure'], r['axes'].get('verified')) for r in json.load(open('/tmp/claude-1000/verify.json'))])"
```
Expected: three cells, each `True`. If any cell fails verification, the `expected: 4` in `triangle.yml` is wrong for the committed data — report NEEDS_CONTEXT with the actual count rather than editing the number.

- [ ] **Step 7: fmt, commit**

```bash
git add kermit/src kermit/tests/cli_bench_run_verify.rs benchmarks/triangle.yml
git commit -m "feat(kermit): bench run --verify checks result cardinalities before timing"
```

---

### Task 4 (D): schema-version pin

**Files:**
- Modify: `kermit/src/bench_report.rs` (tests module, ~line 274)

- [ ] **Step 1: Failing test**

Append inside `mod tests`:

```rust
    /// The Python analysis layer keeps its own copy of the schema version.
    /// Embedding its module at compile time makes a mismatch a `cargo test`
    /// failure rather than a notebook surprise.
    const KERMIT_LAB_INIT: &str =
        include_str!("../../python/kermit-lab/kermit_lab/__init__.py");

    fn python_schema_version(source: &str) -> Option<u32> {
        source
            .lines()
            .find_map(|l| l.strip_prefix("SCHEMA_VERSION = "))
            .and_then(|v| v.trim().parse().ok())
    }

    #[test]
    fn python_schema_version_matches_rust() {
        assert_eq!(
            python_schema_version(KERMIT_LAB_INIT),
            Some(REPORT_SCHEMA_VERSION),
            "python/kermit-lab/kermit_lab/__init__.py SCHEMA_VERSION must equal \
             kermit/src/bench_report.rs REPORT_SCHEMA_VERSION; bump both together"
        );
    }

    #[test]
    fn python_schema_version_parser_rejects_a_file_without_the_line() {
        assert_eq!(python_schema_version("# nothing here\nX = 2\n"), None);
    }
```

- [ ] **Step 2: Run**

`CARGO_BUILD_JOBS=2 cargo test -p kermit --bin kermit bench_report::tests::python_schema` — both pass immediately (the constants already agree at 2). To prove the pin bites, temporarily edit the Python file to `SCHEMA_VERSION = 3`, rerun (must FAIL with the message), then `git checkout python/kermit-lab/kermit_lab/__init__.py`.

- [ ] **Step 3: fmt, commit**

```bash
git add kermit/src/bench_report.rs
git commit -m "test(kermit): pin the Python schema version to REPORT_SCHEMA_VERSION"
```

---

### Task 5 (D): `verified` column and the contract test

**Files:**
- Modify: `python/kermit-lab/kermit_lab/frame.py`, `python/kermit-lab/tests/conftest.py`, `python/kermit-lab/tests/test_frame.py`, `python/kermit-lab/README.md`
- Create: `python/kermit-lab/tests/test_contract.py`

All Python commands run from `python/kermit-lab/` with `uv run …` (first `uv sync --group test`; inside `nix develop` on NixOS so the wheels load).

- [ ] **Step 1: Failing tests**

`python/kermit-lab/tests/conftest.py`: add a fixture after `fixture_tree`:

```python
@pytest.fixture
def verified_tree(tmp_path: Path) -> dict:
    """Two `run` reports on one query: one carries `verified: true`, one no
    `verified` key at all (a run without --verify)."""
    criterion_root = tmp_path / "criterion"
    reports_dir = tmp_path / "reports"
    reports_dir.mkdir()
    paths = []
    for tag, axes_extra in (("v", {"verified": True}), ("nv", {})):
        group = f"run/triangle/triangle/TreeTrie/LeapfrogTriejoin/{tag}"
        _write_function_dir(
            criterion_root,
            _FunctionSpec(group=group, function="iteration", metric="time",
                          point=1000.0, samples=[(1.0, 1000.0)] * 10),
        )
        paths.append(
            _write_report(
                reports_dir,
                f"run-{tag}",
                kind="run",
                axes={
                    "benchmark": "triangle", "query": "triangle",
                    "data_structure": "TreeTrie", "algorithm": "LeapfrogTriejoin",
                    "optimiser": "lexicographic", "tuples": 12, **axes_extra,
                },
                metadata=[],
                groups=[(group, "iteration", "time")],
            )
        )
    return {"paths": paths, "criterion_root": criterion_root}
```

(Match `_FunctionSpec`'s real field list and `_write_function_dir`'s expectations by reading them; adjust the `samples` shape if the helper wants something else.)

`python/kermit-lab/tests/test_frame.py`, append:

```python
def test_verified_column_is_nullable_boolean(verified_tree):
    df = load(verified_tree["paths"], verified_tree["criterion_root"])
    assert "verified" in df.columns
    assert str(df["verified"].dtype) == "boolean"
    by_group = df.set_index("criterion_group")["verified"]
    assert bool(by_group["run/triangle/triangle/TreeTrie/LeapfrogTriejoin/v"]) is True
    assert pd.isna(by_group["run/triangle/triangle/TreeTrie/LeapfrogTriejoin/nv"])
```

Create `python/kermit-lab/tests/test_contract.py`:

```python
"""Contract test: run the real `kermit` binary and load its output.

Skipped unless ``KERMIT_BIN`` names a built binary (CI sets it after
``cargo build -p kermit``). This is the only test that catches a Criterion
JSON-layout change or a Rust-side axis rename.
"""
from __future__ import annotations

import os
import subprocess
from pathlib import Path

import pytest

import kermit_lab as kl

KERMIT_BIN = os.environ.get("KERMIT_BIN")
WORKSPACE = Path(__file__).resolve().parents[3]
FIXTURES = WORKSPACE / "kermit" / "tests" / "fixtures"
FAST = ["--sample-size", "10", "--measurement-time", "1", "--warm-up-time", "1"]

pytestmark = pytest.mark.skipif(not KERMIT_BIN, reason="KERMIT_BIN not set")


def _run(cwd: Path, report: Path, *args: str) -> None:
    subprocess.run(
        [KERMIT_BIN, "bench", *FAST, "--report-json", str(report), *args],
        cwd=cwd, check=True, capture_output=True, text=True,
    )


def test_bench_ds_report_loads(tmp_path: Path) -> None:
    report = tmp_path / "ds.json"
    _run(tmp_path, report, "ds", "--relation", str(FIXTURES / "edge.csv"),
         "-i", "tree-trie", "-m", "space")
    df = kl.load(report, criterion_root=tmp_path / "target" / "criterion")
    assert len(df) == 1
    row = df.iloc[0]
    assert row["kind"] == "ds"
    assert row["metric"] == "space"
    assert row["data_structure"] == "TreeTrie"
    assert row["mean_ns"] > 0


def test_bench_run_verify_reaches_the_frame(tmp_path: Path) -> None:
    report = tmp_path / "run.json"
    _run(WORKSPACE, report, "run", "triangle", "-i", "tree-trie",
         "-a", "leapfrog-triejoin", "-m", "iteration", "--verify")
    df = kl.load(report, criterion_root=WORKSPACE / "target" / "criterion")
    assert len(df) == 1
    row = df.iloc[0]
    assert row["benchmark"] == "triangle"
    assert row["query"] == "triangle"
    assert bool(row["verified"]) is True
```

- [ ] **Step 2: Verify failure**

`uv run pytest tests/test_frame.py::test_verified_column_is_nullable_boolean` — FAIL (`verified` not in columns). `KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest tests/test_contract.py` (after `CARGO_BUILD_JOBS=2 cargo build -p kermit` at the workspace root) — the second test FAILS on `verified` (KeyError or NA).

- [ ] **Step 3: Implement**

`frame.py`: after `_AXIS_INT_KEYS` add

```python
_AXIS_BOOL_KEYS: tuple[str, ...] = (
    "verified",
)
```

include `*_AXIS_BOOL_KEYS` in `_SUMMARY_COLUMNS_CORE` after the int keys; in `_summary_row` add

```python
    for key in _AXIS_BOOL_KEYS:
        v = report.axis(key)
        row[key] = v if isinstance(v, bool) else pd.NA
```

and in `_summary_from_reports` after the `Int64` cast:

```python
    for key in _AXIS_BOOL_KEYS:
        df[key] = df[key].astype("boolean")
```

Update the comment above `_AXIS_STR_KEYS` to mention the three tuples. `test_summary_columns_present` in `test_frame.py`: add `"verified"` to its expected set.

`python/kermit-lab/README.md`, "Schema" section: after the sentence listing fixed axis columns add: "`verified` (nullable boolean) is present when `bench run --verify` checked the query's answer count." And a new short section before "Style":

```markdown
## Contract test

`tests/test_contract.py` runs the real `kermit` binary and loads its output.
It is skipped unless `KERMIT_BIN` points at a built binary:

    cargo build -p kermit
    KERMIT_BIN=$PWD/target/debug/kermit uv run --project python/kermit-lab pytest

CI runs it on every pull request.
```

- [ ] **Step 4: Verify**

`uv run pytest` — all pass (contract tests skipped). `KERMIT_BIN=<workspace>/target/debug/kermit uv run pytest tests/test_contract.py -v` — 2 passed.

- [ ] **Step 5: Commit**

```bash
git add python/kermit-lab
git commit -m "feat(kermit-lab): verified column and a real-binary contract test"
```

---

### Task 6 (D): CI Python job

**Files:**
- Modify: `.github/workflows/pr.yml`, `.github/workflows/build.yml`

- [ ] **Step 1: Resolve the action pin**

The repo pins actions by commit SHA with a version comment. Resolve `astral-sh/setup-uv`'s SHA for its latest v6 tag:

```bash
gh api repos/astral-sh/setup-uv/releases/latest --jq .tag_name
gh api "repos/astral-sh/setup-uv/git/ref/tags/$(gh api repos/astral-sh/setup-uv/releases/latest --jq .tag_name)" --jq '.object | "\(.sha) \(.type)"'
```

If the type is `tag` (annotated), dereference: `gh api repos/astral-sh/setup-uv/git/tags/<sha> --jq .object.sha`. Use the resulting commit SHA.

- [ ] **Step 2: Add the job**

Append to `jobs:` in `pr.yml`:

```yaml
  python:
    name: kermit-lab tests (with real-binary contract test)
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
      - uses: dtolnay/rust-toolchain@efa25f7f19611383d5b0ccf2d1c8914531636bf9 # master
        with:
          toolchain: nightly
      - uses: Swatinem/rust-cache@ad397744b0d591a723ab90405b7247fac0e6b8db # v2
      - run: cargo build -p kermit
      - uses: astral-sh/setup-uv@<SHA> # <tag>
      - run: uv sync --group test
        working-directory: python/kermit-lab
      - run: uv run pytest
        working-directory: python/kermit-lab
        env:
          KERMIT_BIN: ${{ github.workspace }}/target/debug/kermit
```

Add the same job to `build.yml` (its jobs use the same step style; place it after `check`).

- [ ] **Step 3: Validate the YAML locally**

Run `python3 -c "import yaml; [yaml.safe_load(open(f)) for f in ['.github/workflows/pr.yml', '.github/workflows/build.yml']]; print('ok')"`. If that Python lacks PyYAML, try `nix develop --command python3 -c "…"` with the same snippet; if neither has it, skip local validation and say so in the report (CI itself validates on the next push). Do not add PyYAML to any project. Then `grep -n 'setup-uv' .github/workflows/*.yml` must show the same pinned SHA in both files.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/pr.yml .github/workflows/build.yml
git commit -m "ci: run kermit-lab tests with the real-binary contract test"
```

---

### Task 7 (E): `sha256` on relations, digest checks in `kermit-bench`

**Files:**
- Modify: `kermit-bench/src/definition.rs`, `kermit-bench/src/error.rs`, `kermit-bench/src/cache.rs`

- [ ] **Step 1: Failing tests**

`kermit-bench/src/cache.rs` `mod tests`, append:

```rust
    #[test]
    fn sha256_hex_matches_a_known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc.txt");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_hex(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn check_digest_accepts_match_and_reports_mismatch() {
        let ok = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(check_digest(b"abc", ok), Ok(()));
        let bad = "0".repeat(64);
        assert_eq!(check_digest(b"abc", &bad), Err(ok.to_string()));
    }

    fn local_bench(root: &std::path::Path, sha256: Option<&str>) -> BenchmarkDefinition {
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/edge.csv"), b"abc").unwrap();
        BenchmarkDefinition {
            name: "local".to_string(),
            description: String::new(),
            relations: vec![RelationSource {
                name: "edge".to_string(),
                url: None,
                path: Some("data/edge.csv".to_string()),
                sha256: sha256.map(str::to_string),
            }],
            queries: vec![],
            generator: None,
        }
    }

    #[test]
    fn verify_integrity_counts_checked_relations() {
        let root = tempfile::tempdir().unwrap();
        let ok = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(verify_integrity(&local_bench(root.path(), Some(ok)), root.path()).unwrap(), 1);
        assert_eq!(verify_integrity(&local_bench(root.path(), None), root.path()).unwrap(), 0);
    }

    #[test]
    fn verify_integrity_reports_a_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let bad = "0".repeat(64);
        let err = verify_integrity(&local_bench(root.path(), Some(&bad)), root.path()).unwrap_err();
        match err {
            | BenchError::Integrity { relation, expected, actual, .. } => {
                assert_eq!(relation, "edge");
                assert_eq!(expected, bad);
                assert_eq!(actual, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
            },
            | other => panic!("expected Integrity, got {other:?}"),
        }
    }
```

(`mod tests` may need `use crate::{BenchmarkDefinition, RelationSource};` — add to its `use`.) `kermit-bench/src/definition.rs` `mod tests`, append:

```rust
    #[test]
    fn relation_sha256_must_be_64_lowercase_hex() {
        let good = "a".repeat(64);
        let upper = "A".repeat(64);
        let short = "a".repeat(63);
        let non_hex = "g".repeat(64);
        let cases: [(&str, bool); 4] =
            [(&good, true), (&upper, false), (&short, false), (&non_hex, false)];
        for (digest, ok) in cases {
            let mut def = valid_static_definition();
            def.relations[0].sha256 = Some(digest.to_string());
            assert_eq!(def.validate().is_ok(), ok, "{digest}");
        }
    }
```

`valid_static_definition()` stands for whatever helper the tests module already uses to build a definition that passes `validate()` (grep `fn .*def` in that module; the materialize tests call theirs `static_def`). Use the existing helper's real name; if none builds a *valid* static definition with at least one relation, add one.

- [ ] **Step 2: Verify failure**

`CARGO_BUILD_JOBS=2 cargo test -p kermit-bench sha256 check_digest verify_integrity relation_sha256` — compile errors.

- [ ] **Step 3: Implement**

`definition.rs`: make `hex_digest` `pub(crate)`. Add to `RelationSource` after `path`:

```rust
    /// Optional SHA-256 of the relation file, 64 lowercase hex characters.
    /// Checked after a download and by `bench fetch`; never on `bench run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
```

Add `sha256: None,` to every `RelationSource { … }` literal the compiler reports across the workspace (`yaml_emit.rs`, tests in `materialize.rs`, `workload.rs`, `cache.rs`, `definition.rs`). In `validate_relation_source`, before the `url`/`path` match:

```rust
        if let Some(digest) = &rel.sha256 {
            let well_formed =
                digest.len() == 64 && digest.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            if !well_formed {
                return Err(invalid(format!(
                    "relation '{}' sha256 must be 64 lowercase hex characters",
                    rel.name
                )));
            }
        }
```

`error.rs`: add the variant

```rust
    /// A relation file's contents do not match the `sha256` its benchmark
    /// declares.
    #[error(
        "integrity check failed for relation '{relation}' from {location}: expected sha256 \
         {expected}, got {actual}"
    )]
    Integrity {
        /// Relation name as declared in the YAML.
        relation: String,
        /// The URL it was downloaded from, or the local path that was hashed.
        location: String,
        /// Digest declared in the YAML.
        expected: String,
        /// Digest computed from the bytes.
        actual: String,
    },
```

`cache.rs`: add

```rust
/// Streaming SHA-256 of a file, as 64 lowercase hex characters.
///
/// # Errors
///
/// Returns [`BenchError::Io`] if the file cannot be read.
pub fn sha256_hex(path: &Path) -> Result<String, BenchError> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(crate::definition::hex_digest(&hasher.finalize()))
}

/// Compares `bytes` against a declared digest. `Err` carries the actual
/// digest so the caller can report both.
fn check_digest(bytes: &[u8], expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let actual = crate::definition::hex_digest(&Sha256::digest(bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(actual)
    }
}

/// Re-hashes every relation of `benchmark` that declares a `sha256`
/// (cached `url:` files and committed `path:` files alike) and returns how
/// many were checked. Callers run [`ensure_cached`] first.
///
/// # Errors
///
/// [`BenchError::Integrity`] on the first mismatch; [`BenchError::Io`] if a
/// file is missing or unreadable; [`BenchError::NoCacheDir`] as usual.
pub fn verify_integrity(
    benchmark: &BenchmarkDefinition, workspace_root: &Path,
) -> Result<usize, BenchError> {
    let mut checked = 0;
    for rel in &benchmark.relations {
        let Some(expected) = &rel.sha256 else {
            continue;
        };
        let path = match &rel.path {
            | Some(local) => workspace_root.join(local),
            | None => relation_cache_path(&benchmark.name, &rel.name)?,
        };
        let actual = sha256_hex(&path)?;
        if &actual != expected {
            return Err(BenchError::Integrity {
                relation: rel.name.clone(),
                location: path.display().to_string(),
                expected: expected.clone(),
                actual,
            });
        }
        checked += 1;
    }
    Ok(checked)
}
```

`download_file` becomes `fn download_file(url: &str, dest: &Path, relation: &str, expected: Option<&str>) -> Result<(), BenchError>`; after `let bytes = …;` and before `fs::write(&part_path, &bytes)?;` insert:

```rust
    if let Some(expected) = expected {
        if let Err(actual) = check_digest(&bytes, expected) {
            return Err(BenchError::Integrity {
                relation: relation.to_string(),
                location: url.to_string(),
                expected: expected.to_string(),
                actual,
            });
        }
    }
```

(Checking before writing the `.part` file means nothing is left behind on mismatch; keep the existing rename.) In `ensure_cached` the call becomes `download_file(url, &path, &rel.name, rel.sha256.as_deref())?;` with the comment "A declared digest is checked before the file reaches its cache path." Update the `ensure_cached` doc's error list to add `BenchError::Integrity`. Update the module-level doc of `cache.rs` if it describes the download flow.

Check `io` is imported in `cache.rs` (`std::io`); `io::copy` needs `Read`/`Write` in scope — `std::io::copy` works on `File` and on `Sha256` (which implements `Write` via `sha2`'s `std` feature; if the compiler says `Sha256: Write` is unsatisfied, enable `features = ["std"]` on `sha2` in `kermit-bench/Cargo.toml`).

- [ ] **Step 4: Verify**

`CARGO_BUILD_JOBS=2 cargo test -p kermit-bench` — all pass (6 new). `CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets` clean (fix any `RelationSource` literals). `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets -- -D warnings` clean.

- [ ] **Step 5: fmt, commit**

```bash
git add -A kermit-bench kermit-rdf kermit
git commit -m "feat(kermit-bench): optional sha256 on relations, checked on download and by verify_integrity"
```

---

### Task 8 (E): `bench fetch` verifies; pin `triangle`

**Files:**
- Modify: `kermit/src/main.rs` (`run_fetch`, ~line 706), `benchmarks/triangle.yml`
- Create: `kermit/tests/cli_bench_fetch_integrity.rs`

- [ ] **Step 1: Failing CLI tests**

Create `kermit/tests/cli_bench_fetch_integrity.rs`:

```rust
//! `bench fetch` re-hashes every relation that declares a `sha256`.
//! Uses a fake cache whose relation file already exists, so no download
//! happens and the check runs on the cached bytes.

use {
    std::{fs, path::PathBuf, process::Command},
    tempfile::TempDir,
};

fn kermit_bin() -> PathBuf { PathBuf::from(env!("CARGO_BIN_EXE_kermit")) }

fn fixtures_dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures") }

fn skip_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) {
        eprintln!("skipping cli_bench_fetch_integrity test: XDG_CACHE_HOME needs Linux");
        return true;
    }
    false
}

/// A one-relation cache-side benchmark `pinned` whose `parentcountry`
/// parquet is copied from the fixture and declared with `sha256`.
fn make_cache(sha256: &str) -> TempDir {
    let cache = tempfile::tempdir().unwrap();
    let dir = cache.path().join("kermit/benchmarks/pinned");
    fs::create_dir_all(&dir).unwrap();
    let src = fixtures_dir().join("watdiv-mini/artifacts/parentcountry.parquet");
    fs::copy(&src, dir.join("parentcountry.parquet")).unwrap();
    fs::write(
        dir.join("benchmark.yml"),
        format!(
            "name: pinned\n\
             description: integrity fixture\n\
             relations:\n\
             - name: parentcountry\n  \
               url: file:///fixture/parentcountry.parquet\n  \
               sha256: {sha256}\n\
             queries:\n\
             - name: q\n  \
               description: q\n  \
               query: 'Q(X, Y) :- parentcountry(X, Y).'\n"
        ),
    )
    .unwrap();
    fs::write(dir.join("meta.json"), "{}").unwrap();
    cache
}

fn fetch(cache: &TempDir) -> std::process::Output {
    let workspace = tempfile::tempdir().unwrap();
    Command::new(kermit_bin())
        .env("XDG_CACHE_HOME", cache.path())
        .env("KERMIT_WORKSPACE", workspace.path())
        .args(["bench", "fetch", "pinned"])
        .output()
        .unwrap()
}

#[test]
fn fetch_verifies_a_correct_digest() {
    if skip_unsupported() {
        return;
    }
    let digest = kermit_bench::cache::sha256_hex(
        &fixtures_dir().join("watdiv-mini/artifacts/parentcountry.parquet"),
    )
    .unwrap();
    let output = fetch(&make_cache(&digest));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("Verified 1 relation(s)"), "{stderr}");
}

#[test]
fn fetch_rejects_a_wrong_digest() {
    if skip_unsupported() {
        return;
    }
    let wrong = "0".repeat(64);
    let output = fetch(&make_cache(&wrong));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr: {stderr}");
    assert!(stderr.contains("integrity check failed for relation 'parentcountry'"), "{stderr}");
    assert!(stderr.contains(&wrong), "{stderr}");
}
```

`kermit_bench` is already a dependency of `kermit`, so the test can call `kermit_bench::cache::sha256_hex`.

- [ ] **Step 2: Verify failure**

`CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_fetch_integrity` — `fetch_verifies_a_correct_digest` fails (no `Verified` line); `fetch_rejects_a_wrong_digest` fails (exit 0, nothing checked).

- [ ] **Step 3: Implement**

`run_fetch` becomes:

```rust
fn run_fetch(name: Option<String>) -> anyhow::Result<()> {
    let benchmarks = resolve_benchmarks(&name, name.is_none())?;
    for benchmark in &benchmarks {
        eprintln!("Fetching {}...", benchmark.name);
        let root = workspace_root();
        kermit_bench::cache::ensure_cached(benchmark, &root)
            .map_err(|e| anyhow::anyhow!("Failed to fetch {}: {e}", benchmark.name))?;
        let checked = kermit_bench::cache::verify_integrity(benchmark, &root)
            .map_err(|e| anyhow::anyhow!("Failed to verify {}: {e}", benchmark.name))?;
        if checked == 0 {
            eprintln!("  No integrity hashes declared.");
        } else {
            eprintln!("  Verified {checked} relation(s).");
        }
        eprintln!("  Done.");
    }
    Ok(())
}
```

`benchmarks/triangle.yml`: under the `edge` relation add `    sha256: "942b63a8faab8c2241f1df373bde53b8dc629e992be4a8eee5b1e9514d84862e"` (recompute with `sha256sum benchmarks/data/triangle/edge.csv` and use that value if it differs).

- [ ] **Step 4: Verify**

`CARGO_BUILD_JOBS=2 cargo test -p kermit --test cli_bench_fetch_integrity` — 2 passed. `CARGO_BUILD_JOBS=2 cargo run -q -- bench fetch triangle` prints `Verified 1 relation(s).` `CARGO_BUILD_JOBS=2 cargo test -p kermit` all green; clippy clean.

- [ ] **Step 5: fmt, commit**

```bash
git add kermit/src/main.rs kermit/tests/cli_bench_fetch_integrity.rs benchmarks/triangle.yml
git commit -m "feat(kermit): bench fetch verifies declared sha256 digests; pin triangle's relation"
```

---

### Task 9: Docs, spec status, full gate

**Files:** `benchmarks/README.md`, `USAGE.md`, `BENCHMARKING.md`, `CLAUDE.md`, `docs/benchmarks/LUBM.md`, `docs/benchmarks/WATDIV.md`, `docs/specs/bench-report-schema.md`, `docs/specs/benchmarking-architecture.md`, `python/kermit-lab/README.md` (done in Task 5), `docs/specs/2026-09-09-answer-verification-design.md`.

- [ ] **Step 1: `benchmarks/README.md`**

In the "Static benchmark fields" schema section, add to the query fields: "`expected` (optional integer): the query's result cardinality. `bench run --verify` runs the query once and aborts on a mismatch." and to the relation fields: "`sha256` (optional, 64 lowercase hex): digest of the relation file, checked after download and by `bench fetch`. Compute with `sha256sum <file>`." Update the "Minimal example (`triangle.yml`)" block to show both new keys (copy the real file). In the "Frozen snapshots" section, replace the sentence claiming the preprocessor is "the only source of WatDiv expected cardinalities" with: "Expected cardinalities for these snapshots are not yet in the YAMLs; the preprocessor's `expected.json` values could be folded in later."

- [ ] **Step 2: `USAGE.md`**

`bench run` section: document `--verify` ("runs each query once before timing and compares its result count with the benchmark's `expected`; mismatch aborts; queries without `expected` are noted"). `bench fetch` section: "also re-hashes every relation with a declared `sha256`."

- [ ] **Step 3: `BENCHMARKING.md`**

Add a short section "Verify before you trust" after "The loop": before a measurement campaign, run each cell once with `--verify` (`bench run <name> -i all -a all -m iteration --verify`); a benchmark whose queries carry `expected` then cannot produce timings for wrong answers; explain that it is opt-in because it costs one extra query execution per cell.

- [ ] **Step 4: `CLAUDE.md`**

- JSON bench reports gotcha: add "`verified: true` appears in `axes` only when `bench run --verify` checked the query."
- LUBM on-the-fly gotcha: change "exposed with LUBM(1, 0) reference cardinalities (paper Table 3)" to say they are attached as `expected` in the emitted YAML only when `lubm_reference_applies(scale, seed, start_index)` (scale 1, seed 0, start index 0), and remove any mention of `expected/*.csv`.
- Workspace Architecture `kermit-rdf` entry: in the "Shared stages" list replace `expected` with nothing (delete it) and mention that `yaml_emit` writes `expected` cardinalities.
- WatDiv on-the-fly driver gotcha: replace "the vendored binary emits no `.desc` cardinality sidecars, so `expected/*.csv` is empty for now" with "the vendored binary emits no `.desc` cardinality sidecars, so WatDiv queries carry no `expected` value".
- Add a gotcha "**Relation integrity**: `RelationSource.sha256` is optional; `ensure_cached` checks it after a download (before the file reaches the cache path) and `bench fetch` re-hashes cached and committed files; `bench run` never hashes."
- CI Checks list: add "the `python` job runs `kermit-lab`'s pytest with `KERMIT_BIN` set, including the real-binary contract test".

- [ ] **Step 5: `docs/benchmarks/LUBM.md`, `WATDIV.md`, specs**

`LUBM.md`: the reference-cardinality table stays; state they are attached as `expected` only for LUBM(1, 0) and verified with `--verify`. `WATDIV.md`: remove the sidecar description. `bench-report-schema.md`: add the row `| verified | run | boolean | true when --verify checked the query's result count; absent otherwise |` and note "schema stays 2". `benchmarking-architecture.md`: `bench run` arguments gain `--verify`; flow step "run the query once and compare with `expected` when `--verify`"; `bench fetch` bullet gains "and verifies declared digests".

- [ ] **Step 6: spec status and gate**

Set `**Status:** Implemented 2026-09-09` in `docs/specs/2026-09-09-answer-verification-design.md`. Run inside `nix develop`:

```bash
nix develop --command bash -c 'cargo fmt --all --check && CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets && RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=2 cargo doc --workspace --no-deps && CARGO_BUILD_JOBS=2 cargo test && cd python/kermit-lab && uv sync --group test && KERMIT_BIN=$PWD/../../target/debug/kermit uv run pytest'
```

Manual: `bench run triangle -i all -a all -m iteration --verify` (three `verified: true`); `bench fetch triangle` (`Verified 1 relation(s).`); and the failure message by eye: copy `benchmarks/triangle.yml` and `benchmarks/data/` into a temp workspace, set `expected: 5`, run with `KERMIT_WORKSPACE=<tmp>`, confirm `verification failed … returned 4 tuples, expected 5`.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "docs: expected cardinalities, --verify, sha256 integrity, and the kermit-lab contract test"
```
