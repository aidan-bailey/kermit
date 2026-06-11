# WatDiv Default Workloads Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make WatDiv's default workloads — the Basic Testing query set (L/S/F/C) and the stress workload at default parameters — available as generator-driven `bench run` benchmark suites.

**Architecture:** Add a `kind: watdiv-basic` `GeneratorSpec` variant. A new `kermit-rdf` basic pipeline runs `watdiv -d` then feeds the 20 vendored static templates to `watdiv -q` (skipping `-s`), reusing every downstream stage (partition → parquet → translate → YAML → meta). Two new benchmark YAMLs route through the existing `materialize` cache/spec-hash layer. Generation is non-deterministic by design (see the spec's recorded trade-off).

**Tech Stack:** Rust (nightly), `serde`/`serde_yaml`, the vendored WatDiv C++ binary, `bwrap` sandbox.

**Spec:** `docs/specs/2026-06-11-watdiv-default-workloads-design.md`

---

## File structure

- `kermit-rdf/vendor/watdiv/testsuite/*.txt` — **new**, 20 committed Basic Testing templates.
- `kermit-bench/src/definition.rs` — **modify**, add `GeneratorSpec::WatdivBasic` + validation.
- `kermit-rdf/src/driver/mod.rs` — **modify**, add `drive_basic`.
- `kermit-rdf/src/pipeline.rs` — **modify**, add `run_basic_pipeline`; parameterize `process_artifacts` with `meta_kind`.
- `kermit-rdf/tests/pipeline.rs` — **modify**, update the `process_artifacts` call site.
- `kermit/src/materialize.rs` — **modify**, dispatch the new variant via `run_watdiv_basic`.
- `benchmarks/watdiv-basic.yml`, `benchmarks/watdiv-stress-default.yml` — **new**.
- `kermit-rdf/tests/e2e_watdiv_basic.rs` — **new**, gated integration test.
- `docs/benchmarks/WATDIV.md` — **modify**, document templates + workloads.

**Out of scope (deliberate):** the imperative `bench gen watdiv --workload basic` CLI flag (the deliverable is `bench run`, which needs only the YAML path), a WatDiv cardinality oracle, and forwarding the unused stress params.

---

### Task 1: Vendor the Basic Testing templates

**Files:**
- Create: `kermit-rdf/vendor/watdiv/testsuite/{C1..C3,F1..F5,L1..L5,S1..S7}.txt` (20 files)

- [ ] **Step 1: Copy the 20 templates from upstream**

Run:
```bash
mkdir -p kermit-rdf/vendor/watdiv/testsuite
cp /tb/Source/Academia/watdiv-rs/.loom/worktrees/aidanb/rust-rewrite_18ac965cb0f10468/watdiv-cpp/testsuite/*.txt \
   kermit-rdf/vendor/watdiv/testsuite/
```

- [ ] **Step 2: Verify exactly 20 templates landed**

Run: `ls kermit-rdf/vendor/watdiv/testsuite/*.txt | wc -l`
Expected: `20`

Run: `ls kermit-rdf/vendor/watdiv/testsuite/ | sort -V | tr '\n' ' '`
Expected: `C1.txt C2.txt C3.txt F1.txt F2.txt F3.txt F4.txt F5.txt L1.txt L2.txt L3.txt L4.txt L5.txt S1.txt S2.txt S3.txt S4.txt S5.txt S6.txt S7.txt`

- [ ] **Step 3: Sanity-check one template is a valid `-q` template**

Run: `head -1 kermit-rdf/vendor/watdiv/testsuite/S1.txt`
Expected: starts with `#mapping ` (placeholder declaration line).

- [ ] **Step 4: Commit**

```bash
git add kermit-rdf/vendor/watdiv/testsuite
git commit -m "feat(rdf): vendor WatDiv Basic Testing query templates (L/S/F/C)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Add the `WatdivBasic` GeneratorSpec variant

**Files:**
- Modify: `kermit-bench/src/definition.rs` (enum near line 43; `validate_generator` near line 287)
- Test: inline `#[cfg(test)]` in the same file

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `kermit-bench/src/definition.rs`:

```rust
#[test]
fn deserialize_watdiv_basic_generator() {
    let yaml = r#"
name: watdiv-basic
description: "watdiv basic"
generator:
  kind: watdiv-basic
  scale: 10
"#;
    let def: BenchmarkDefinition = serde_yaml::from_str(yaml).unwrap();
    def.validate().unwrap();
    match def.generator.as_ref().unwrap() {
        | GeneratorSpec::WatdivBasic { scale } => assert_eq!(*scale, 10),
        | other => panic!("expected WatdivBasic, got {other:?}"),
    }
}

#[test]
fn watdiv_basic_scale_zero_is_invalid() {
    let spec = GeneratorSpec::WatdivBasic { scale: 0 };
    assert!(validate_generator("b", &spec).is_err());
}

#[test]
fn watdiv_basic_spec_hash_differs_from_watdiv() {
    let basic = GeneratorSpec::WatdivBasic { scale: 10 };
    let stress = GeneratorSpec::Watdiv { scale: 10, stress: WatdivStressSpec::default() };
    assert_ne!(basic.spec_hash(), stress.spec_hash());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kermit-bench deserialize_watdiv_basic_generator`
Expected: FAIL — compile error, `GeneratorSpec` has no variant `WatdivBasic`.

- [ ] **Step 3: Add the variant**

In `kermit-bench/src/definition.rs`, inside `enum GeneratorSpec` (after the `Watdiv { … }` variant, before `Lubm`):

```rust
    /// Drives the WatDiv Basic Testing workload — the 20 canonical L/S/F/C
    /// query templates (`kermit_rdf::pipeline::run_basic_pipeline`). Carries
    /// no stress parameters: the templates are fixed.
    WatdivBasic {
        /// Scale factor passed to `watdiv -d` (>= 1).
        scale: u32,
    },
```

- [ ] **Step 4: Add the validation arm**

In `validate_generator`, add a match arm alongside the `Watdiv` arm:

```rust
        | GeneratorSpec::WatdivBasic { scale } => {
            if *scale == 0 {
                return Err(BenchError::Invalid {
                    name: bench_name.to_string(),
                    reason: "watdiv-basic generator scale must be >= 1".to_string(),
                });
            }
        },
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p kermit-bench watdiv_basic`
Expected: PASS (all three new tests).

- [ ] **Step 6: Commit**

```bash
git add kermit-bench/src/definition.rs
git commit -m "feat(bench): add watdiv-basic GeneratorSpec variant

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Add `drive_basic` to the WatDiv driver

**Files:**
- Modify: `kermit-rdf/src/driver/mod.rs` (after `drive`, ~line 109)

- [ ] **Step 1: Add `drive_basic`**

Append to `kermit-rdf/src/driver/mod.rs`:

```rust
/// Like [`drive`], but for the **Basic Testing** workload: runs `-d`, then
/// feeds the static templates in `template_src_dir` (the vendored
/// `testsuite/*.txt`) directly to `-q`, skipping the `-s` stress step.
///
/// The templates are copied into the staging dir first so the `-q` `.sparql`
/// outputs land in the stage rather than polluting the read-only vendored
/// source. `inputs.stress` is ignored (Basic templates carry their own
/// `#mapping` lines); it is retained only so `process_artifacts` can record
/// provenance.
pub fn drive_basic(
    inputs: &DriverInputs, template_src_dir: &Path,
) -> Result<RawArtifacts, RdfError> {
    if !inputs.watdiv_bin.exists() {
        return Err(RdfError::BinaryNotFound {
            path: inputs.watdiv_bin.to_path_buf(),
        });
    }
    let stage = sandbox::TempStagingDir::create(inputs.watdiv_bin, inputs.vendor_files)?;
    let cfg = invoke::InvokeConfig {
        stage: &stage,
        model_file: inputs.model_file,
        use_bwrap: inputs.use_bwrap,
    };

    let bin_release = stage.binary_path().parent().unwrap().to_path_buf();
    let data_nt = bin_release.join("data.nt");
    invoke::run_data(&cfg, inputs.scale, &data_nt)?;

    let tpl_dir = bin_release.join("basic-templates");
    std::fs::create_dir_all(&tpl_dir)?;
    let mut sources: Vec<PathBuf> = std::fs::read_dir(template_src_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("txt"))
        .collect();
    sources.sort();
    if sources.is_empty() {
        return Err(RdfError::Sandbox(format!(
            "no .txt templates found in {template_src_dir:?}"
        )));
    }
    let mut templates = Vec::with_capacity(sources.len());
    for src in &sources {
        let dst = tpl_dir.join(src.file_name().unwrap());
        std::fs::copy(src, &dst)?;
        templates.push(dst);
    }

    let queries = invoke::run_queries(&cfg, &templates, inputs.query_count_per_template)?;

    Ok(RawArtifacts {
        data_nt,
        templates,
        queries,
        stage,
    })
}
```

- [ ] **Step 2: Verify the crate still builds**

Run: `cargo build -p kermit-rdf`
Expected: builds clean (no warnings).

Note: `drive_basic` requires the vendored binary + sandbox to exercise, so it is covered by the gated integration test in Task 7, not a unit test.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/driver/mod.rs
git commit -m "feat(rdf): add drive_basic for the WatDiv Basic Testing workload

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Add `run_basic_pipeline`; parameterize `process_artifacts` with `meta_kind`

**Files:**
- Modify: `kermit-rdf/src/pipeline.rs` (`process_artifacts` ~line 114; `run_pipeline` ~line 217)
- Modify: `kermit-rdf/tests/pipeline.rs` (the `process_artifacts` call site)

- [ ] **Step 1: Parameterize `process_artifacts`**

Change the signature of `process_artifacts` in `kermit-rdf/src/pipeline.rs`:

```rust
pub fn process_artifacts(
    inputs: &PipelineInputs, raw: &RawArtifacts, meta_kind: &str,
) -> Result<PipelineMeta, RdfError> {
```

And change the `kind` field in the `PipelineMeta { … }` literal from `kind: "watdiv-onthefly".to_string(),` to:

```rust
        kind: meta_kind.to_string(),
```

- [ ] **Step 2: Update `run_pipeline` and add `run_basic_pipeline`**

Replace the `run_pipeline` body and append `run_basic_pipeline`:

```rust
/// Top-level entry point: runs the stress driver and processes artifacts.
pub fn run_pipeline(inputs: &PipelineInputs) -> Result<PipelineMeta, RdfError> {
    let raw = driver::drive(&inputs.driver)?;
    process_artifacts(inputs, &raw, "watdiv-onthefly")
}

/// Top-level entry point for the **Basic Testing** workload: runs the basic
/// driver (static templates in `template_src_dir`) and processes artifacts.
pub fn run_basic_pipeline(
    inputs: &PipelineInputs, template_src_dir: &Path,
) -> Result<PipelineMeta, RdfError> {
    let raw = driver::drive_basic(&inputs.driver, template_src_dir)?;
    process_artifacts(inputs, &raw, "watdiv-basic-onthefly")
}
```

(`driver::drive_basic` is already imported transitively via `crate::driver`; if the `use` at the top imports specific driver items, add `drive_basic` to it.)

- [ ] **Step 3: Find and fix every other `process_artifacts` caller**

Run: `grep -rn "process_artifacts" kermit-rdf/`
Expected callers: `pipeline.rs` (the two entry points above) and `kermit-rdf/tests/pipeline.rs`.

In `kermit-rdf/tests/pipeline.rs`, update the call to pass the kind, e.g.:

```rust
    let meta = process_artifacts(&inputs, &raw, "watdiv-onthefly").expect("process_artifacts");
```

(Match the existing variable names in that test; only the third argument is new.)

- [ ] **Step 4: Run kermit-rdf tests to verify nothing regressed**

Run: `nix develop --command cargo test -p kermit-rdf 2>&1 | tail -20`
Expected: all existing tests PASS (the `meta_kind` change is behaviour-preserving for the stress path: same `"watdiv-onthefly"` string).

- [ ] **Step 5: Commit**

```bash
git add kermit-rdf/src/pipeline.rs kermit-rdf/tests/pipeline.rs
git commit -m "feat(rdf): add run_basic_pipeline; parameterize process_artifacts meta kind

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Dispatch `WatdivBasic` in the materialize layer

**Files:**
- Modify: `kermit/src/materialize.rs` (`dispatch` ~line 153; add `run_watdiv_basic`)

- [ ] **Step 1: Add the dispatch arm**

In `kermit/src/materialize.rs`, inside `dispatch`'s `match spec`, add an arm (after the `Watdiv` arm):

```rust
        | GeneratorSpec::WatdivBasic { scale } => {
            run_watdiv_basic(*scale, bench_name, out_dir, spec_hash)
        },
```

- [ ] **Step 2: Add `run_watdiv_basic`**

Add after `run_watdiv` in `kermit/src/materialize.rs`:

```rust
fn run_watdiv_basic(
    scale: u32, bench_name: &str, out_dir: &Path, spec_hash: &str,
) -> anyhow::Result<()> {
    let vendor = vendored_watdiv_root();
    let bin = std::env::var_os("KERMIT_WATDIV_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| vendor.join("bin/Release/watdiv"));
    if !bin.exists() {
        anyhow::bail!("watdiv binary not found at {bin:?}");
    }
    let testsuite = vendor.join("testsuite");
    let inputs = kermit_rdf::pipeline::PipelineInputs {
        driver: kermit_rdf::driver::DriverInputs {
            watdiv_bin: &bin,
            vendor_files: &vendor.join("files"),
            model_file: &vendor.join("MODEL.txt"),
            scale,
            // Basic templates ignore stress params; defaults are recorded in
            // meta.json only for provenance.
            stress: kermit_rdf::driver::StressParams::default(),
            // One concrete query per template → the 20 canonical L/S/F/C queries.
            query_count_per_template: 1,
            use_bwrap: std::env::var_os("KERMIT_NO_BWRAP").is_none(),
        },
        out_dir,
        bench_name,
        tag: bench_name,
        spec_hash: Some(spec_hash),
    };
    kermit_rdf::pipeline::run_basic_pipeline(&inputs, &testsuite)
        .map_err(|e| anyhow::anyhow!("watdiv basic pipeline failed: {e}"))?;
    Ok(())
}
```

- [ ] **Step 3: Verify the binary crate builds**

Run: `cargo build -p kermit`
Expected: builds clean. (The `match spec` in `dispatch` is now exhaustive over all three `GeneratorSpec` variants.)

- [ ] **Step 4: Run materialize unit tests**

Run: `cargo test -p kermit --lib materialize`
Expected: existing materialize tests still PASS (cache classification is variant-agnostic).

- [ ] **Step 5: Commit**

```bash
git add kermit/src/materialize.rs
git commit -m "feat(cli): dispatch watdiv-basic generator to run_basic_pipeline

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Add the two benchmark YAMLs

**Files:**
- Create: `benchmarks/watdiv-basic.yml`
- Create: `benchmarks/watdiv-stress-default.yml`

- [ ] **Step 1: Write `benchmarks/watdiv-basic.yml`**

```yaml
name: watdiv-basic
description: "WatDiv Basic Testing workload (L/S/F/C, 20 templates), generated on the fly at scale 10"
generator:
  kind: watdiv-basic
  scale: 10
```

- [ ] **Step 2: Write `benchmarks/watdiv-stress-default.yml`**

```yaml
name: watdiv-stress-default
description: "WatDiv stress workload at default parameters, generated on the fly at scale 10"
generator:
  kind: watdiv
  scale: 10
```

- [ ] **Step 3: Verify both are discovered and valid**

Run: `cargo run --quiet -- bench list 2>&1 | grep -E "watdiv-basic|watdiv-stress-default"`
Expected: both names appear (status `not generated`, since nothing is cached yet).

- [ ] **Step 4: Commit**

```bash
git add benchmarks/watdiv-basic.yml benchmarks/watdiv-stress-default.yml
git commit -m "feat(bench): add watdiv-basic and watdiv-stress-default benchmarks

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Gated end-to-end test for the basic pipeline

**Files:**
- Create: `kermit-rdf/tests/e2e_watdiv_basic.rs`

- [ ] **Step 1: Write the test (full file)**

```rust
//! End-to-end test for the WatDiv Basic Testing pipeline: drives the real
//! vendored binary on the 20 vendored L/S/F/C templates.
//!
//! Skipped on non-Linux, non-x86_64, or hosts without `bwrap` — mirrors
//! `e2e_watdiv`.

use {
    kermit_rdf::{
        driver::{DriverInputs, StressParams},
        pipeline::{run_basic_pipeline, PipelineInputs},
    },
    std::path::PathBuf,
};

fn vendor_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/watdiv") }

fn skip_if_unsupported() -> bool {
    if cfg!(not(target_os = "linux")) || cfg!(not(target_arch = "x86_64")) {
        eprintln!("skipping watdiv-basic e2e: requires linux x86_64");
        return true;
    }
    if std::process::Command::new("bwrap").arg("--version").output().is_err() {
        eprintln!("skipping watdiv-basic e2e: bwrap not found");
        return true;
    }
    let bin = vendor_root().join("bin/Release/watdiv");
    match std::process::Command::new(&bin).output() {
        | Ok(out) if out.status.code() == Some(127) => {
            eprintln!("skipping watdiv-basic e2e: vendored binary missing dynamic deps");
            return true;
        },
        | Err(_) => {
            eprintln!("skipping watdiv-basic e2e: cannot execute vendored binary");
            return true;
        },
        | _ => {},
    }
    let words = vendor_root().join("files/words");
    let bwrap_ok = std::process::Command::new("bwrap")
        .args(["--bind", "/", "/"])
        .args(["--tmpfs", "/usr"])
        .args(["--ro-bind-try", "/usr/bin", "/usr/bin"])
        .args(["--ro-bind-try", "/usr/lib", "/usr/lib"])
        .args(["--ro-bind-try", "/usr/lib64", "/usr/lib64"])
        .args(["--dir", "/usr/share/dict"])
        .arg("--bind")
        .arg(&words)
        .args(["/usr/share/dict/words", "true"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !bwrap_ok {
        eprintln!("skipping watdiv-basic e2e: bwrap cannot bind /usr/share/dict/words");
        return true;
    }
    false
}

#[test]
#[cfg_attr(
    miri,
    ignore = "spawns bwrap/watdiv subprocesses via std::process::Command"
)]
fn watdiv_basic_pipeline_produces_twenty_queries() {
    if skip_if_unsupported() {
        return;
    }
    let vendor = vendor_root();
    let testsuite = vendor.join("testsuite");
    let dir = tempfile::tempdir().unwrap();
    let inputs = PipelineInputs {
        driver: DriverInputs {
            watdiv_bin: &vendor.join("bin/Release/watdiv"),
            vendor_files: &vendor.join("files"),
            model_file: &vendor.join("MODEL.txt"),
            scale: 1,
            stress: StressParams::default(),
            query_count_per_template: 1,
            use_bwrap: true,
        },
        out_dir: dir.path(),
        bench_name: "watdiv-basic-e2e",
        tag: "e2e",
        spec_hash: None,
    };
    let meta = run_basic_pipeline(&inputs, &testsuite).expect("basic pipeline failed");

    assert_eq!(meta.kind, "watdiv-basic-onthefly");
    assert!(meta.triple_count > 0, "no triples generated");
    assert!(meta.relation_count > 0, "no relations partitioned");
    // 20 templates × 1 query each = 20 queries.
    assert_eq!(meta.query_count, 20, "expected one query per L/S/F/C template");

    assert!(dir.path().join("benchmark.yml").exists());
    assert!(dir.path().join("dict.parquet").exists());
    assert!(dir.path().join("meta.json").exists());
}
```

- [ ] **Step 2: Run the test**

Run: `nix develop --command cargo test -p kermit-rdf --test e2e_watdiv_basic -- --nocapture 2>&1 | tail -15`
Expected: PASS on this host (JDK/bwrap present). On unsupported hosts it prints a skip line and passes.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/tests/e2e_watdiv_basic.rs
git commit -m "test(rdf): e2e test for the WatDiv Basic Testing pipeline

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Documentation

**Files:**
- Modify: `docs/benchmarks/WATDIV.md`
- Modify: `docs/specs/2026-06-11-watdiv-default-workloads-design.md` (status)

- [ ] **Step 1: Document the Basic Testing workload + template provenance**

In `docs/benchmarks/WATDIV.md`, add a section:

```markdown
## Default workloads

Two generator-driven benchmarks ship as runnable suites (regenerated on
`bench run`, non-deterministic — see "Non-determinism discipline"):

- **`watdiv-basic`** — the canonical Basic Testing query set: Linear
  (`L1`–`L5`), Star (`S1`–`S7`), Snowflake (`F1`–`F5`), Complex (`C1`–`C3`),
  20 templates. `generator.kind: watdiv-basic`. Runs `watdiv -d` then `-q` on
  the vendored templates (skips `-s`), one concrete query per template.
- **`watdiv-stress-default`** — the diversified stress workload at default
  parameters (`generator.kind: watdiv`, default `WatdivStressSpec`),
  complementing the frozen `watdiv-stress-{100,1000}` snapshots.

Run: `cargo run -- bench run watdiv-basic -i tree-trie -a leapfrog-triejoin`.

### Vendored Basic Testing templates

`kermit-rdf/vendor/watdiv/testsuite/{L,S,F,C}*.txt` (20 files) are the upstream
WatDiv Basic Testing query templates (same provenance as the vendored binary;
see VERSION). Each is a standard `-q` template: a `#mapping` line per
placeholder plus a BGP `SELECT`. `S1`'s `%v2%` is a subject-position
placeholder, so the basic workload exercises the subject-position-constant
join path.
```

- [ ] **Step 2: Mark the spec Accepted**

In `docs/specs/2026-06-11-watdiv-default-workloads-design.md`, change `- **Status:** Proposed` to `- **Status:** Accepted`.

- [ ] **Step 3: Commit**

```bash
git add docs/benchmarks/WATDIV.md docs/specs/2026-06-11-watdiv-default-workloads-design.md
git commit -m "docs(benchmarks): document WatDiv default workloads and vendored templates

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 9: Full verification gate

**Files:** none (verification only)

- [ ] **Step 1: fmt + clippy + workspace tests**

Run:
```bash
nix develop --command cargo fmt --all --check
RUSTFLAGS="-Dwarnings" nix develop --command cargo clippy --all-targets
nix develop --command cargo test 2>&1 | grep -E "test result:|FAILED"
```
Expected: fmt clean; clippy clean; every `test result: ok`, zero `FAILED`.

- [ ] **Step 2: End-to-end smoke of the new benchmark via the CLI**

Run:
```bash
nix develop --command cargo run --quiet -- bench --sample-size 10 --measurement-time 1 --warm-up-time 1 \
  --report-json bench-runs/verify-watdiv-basic.json \
  run watdiv-basic -i tree-trie -a leapfrog-triejoin -m iteration
```
Expected: materializes WatDiv (scale 10), runs the 20 basic queries through Criterion, prints `Report written: bench-runs/verify-watdiv-basic.json`.

- [ ] **Step 3: Confirm the report has 20 query reports**

Run: `jq 'length' bench-runs/verify-watdiv-basic.json`
Expected: `20` (one `BenchReport` per basic query).

---

## Self-review

- **Spec coverage:** Vendored templates (Task 1); `watdiv-basic` variant (Task 2); basic pipeline skipping `-s` (Tasks 3–4); CLI/materialize dispatch (Task 5); both YAMLs (Task 6); gated test exercising the subject-position path via `S1` (Task 7); docs + provenance + non-determinism note (Task 8, spec). Generate-not-freeze and out-of-scope items honoured (no freezing, no oracle, no stress-param forwarding, imperative CLI flag deferred). ✅
- **Type consistency:** `run_basic_pipeline(&PipelineInputs, &Path)`, `drive_basic(&DriverInputs, &Path)`, `process_artifacts(&PipelineInputs, &RawArtifacts, &str)`, `GeneratorSpec::WatdivBasic { scale }`, `run_watdiv_basic(u32, &str, &Path, &str)` — names/signatures match across Tasks 2–7. ✅
- **Placeholder scan:** no TBD/TODO; every code step shows complete code. ✅
