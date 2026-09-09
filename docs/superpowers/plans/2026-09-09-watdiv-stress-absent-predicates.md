# WatDiv Stress Absent-Predicate Seeding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the WatDiv stress pipeline seed an empty relation for every query predicate that drew zero triples, so `e2e_watdiv` stops failing ~13 % of isolated runs (issue #63).

**Architecture:** `kermit-rdf/src/pipeline.rs` already has `seed_missing_predicates`, invoked from `WatdivGenerator::seed_relations` behind a `Workload::Basic`-only gate. The change removes the gate, deletes the now-constant predicate method, and rewords the comments that justified it. Tests pin the seeding function deterministically (no watdiv binary) and relax one e2e assertion that seeding invalidates. Docs record the general behaviour plus the one-second seed-granularity hazard.

**Tech Stack:** Rust nightly (pinned), `cargo test -p kermit-rdf`, `nix develop` for `cargo fmt` and for running the bwrap-backed e2e test. Spec: `docs/superpowers/specs/2026-09-09-watdiv-stress-absent-predicates-design.md`.

**Ground rules for every task:**
- Run cargo foreground with `CARGO_BUILD_JOBS=2` (a background memory monitor kills heavy builds on this machine).
- Never run `cargo fmt` outside `nix develop --command cargo fmt --all` (stable rustfmt rewrites ~30 files).
- Commit messages are conventional commits and end with the attribution trailer given in the session.
- Priorities item 6 (scope discipline): touch only the files listed per task. LUBM, the driver, the translator's error path, and the CLI are out of scope.

---

## File map

| File | Change |
|---|---|
| `kermit-rdf/src/pipeline.rs` | Remove seeding gate; delete `Workload::seeds_missing_predicates`; reword three doc comments and the module doc; add `#[cfg(test)] mod tests` with two unit tests |
| `kermit-rdf/src/generator.rs:178-180` | Reword the `seed_relations` hook doc |
| `kermit-rdf/tests/e2e_watdiv.rs:101-106` | Relax relation-count assertion to `>=`; add YAML-relations-vs-Parquet check |
| `docs/benchmarks/WATDIV.md:44-55` and § Determinism (~line 234) | Generalise absent-predicate section; add seed-granularity paragraph |
| `CLAUDE.md:262` | One sentence on seeding in the WatDiv on-the-fly gotcha |

---

### Task 1: Pin `seed_missing_predicates` with a deterministic unit test

The function already exists and is correct; this test is the regression pin the issue asked for and needs no watdiv, bwrap, or JVM. It should pass immediately.

**Files:**
- Modify: `kermit-rdf/src/pipeline.rs` (append a test module at end of file)

- [ ] **Step 1: Append the test module**

Add at the very end of `kermit-rdf/src/pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::partition::PartitionedRelation,
        std::io::Write,
    };

    /// A `Partitioned` holding one relation, `title`, for `http://a/title`,
    /// as `partition::partition` would build it from data containing that
    /// predicate.
    fn partitioned_with_title() -> Partitioned {
        let mut part = Partitioned::default();
        let s = part.dict.intern(RdfValue::Iri("http://a/s1".into()));
        part.dict.intern(RdfValue::Iri("http://a/title".into()));
        let o = part.dict.intern(RdfValue::Iri("http://a/o1".into()));
        part.relations.push(PartitionedRelation {
            name: "title".into(),
            tuples: vec![(s, o)],
        });
        part.predicate_map
            .insert("http://a/title".into(), "title".into());
        part
    }

    /// Writes `text` to `<dir>/<name>` and returns its path.
    fn write_sparql(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
        path
    }

    #[test]
    fn seed_missing_predicates_adds_empty_relations_for_absent_query_predicates() {
        let dir = tempfile::tempdir().unwrap();
        // One present predicate, one absent predicate, and one absent
        // predicate whose sanitised base (`title`) collides with the
        // present relation's name.
        let sparql = write_sparql(
            dir.path(),
            "q.sparql",
            "SELECT ?s ?o WHERE {\n\
             \t?s <http://a/title> ?o .\n\
             \t?s <http://x/rare> ?r .\n\
             \t?s <http://b/title> ?t .\n\
             }\n#end\n",
        );
        let mut part = partitioned_with_title();
        let dict_len_before = part.dict.len();

        seed_missing_predicates(&mut part, &[sparql]).unwrap();

        // The present relation is untouched.
        assert_eq!(part.relations[0].name, "title");
        assert_eq!(part.relations[0].tuples.len(), 1);
        assert_eq!(part.predicate_map["http://a/title"], "title");

        // Both absent predicates now have an empty relation and a map entry.
        assert_eq!(part.relations.len(), 3, "two relations seeded");
        assert_eq!(part.predicate_map.len(), 3);
        let rare = &part.predicate_map["http://x/rare"];
        assert_eq!(rare, "rare");
        let collided = &part.predicate_map["http://b/title"];
        let collided_id = part
            .dict
            .lookup(&RdfValue::Iri("http://b/title".into()))
            .expect("absent predicate interned into the dictionary");
        assert_eq!(collided, &format!("title_{collided_id}"));
        for name in [rare, collided] {
            let rel = part
                .relations
                .iter()
                .find(|r| &r.name == name)
                .unwrap_or_else(|| panic!("no relation named {name}"));
            assert!(rel.tuples.is_empty(), "{name} must be seeded empty");
        }

        // Exactly the two absent IRIs were interned.
        assert_eq!(part.dict.len(), dict_len_before + 2);
    }
}
```

- [ ] **Step 2: Run the test; it should pass**

```bash
CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf --lib seed_missing_predicates_adds_empty_relations
```
Expected: `test pipeline::tests::seed_missing_predicates_adds_empty_relations_for_absent_query_predicates ... ok`

If it fails on the `title_{id}` assertion, read `seed_missing_predicates` at `kermit-rdf/src/pipeline.rs:167-201`: the suffix is the dictionary ID from `part.dict.intern`, which is what the test looks up. Do not change the function in this task.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/pipeline.rs
git commit -m "test(rdf): pin seed_missing_predicates with a deterministic unit test"
```

---

### Task 2: Seed in the stress workload (the fix)

TDD: first a test that fails on the current code because `Workload::Stress` does not seed, then the change.

**Files:**
- Modify: `kermit-rdf/src/pipeline.rs:136-166` (enum docs, delete method, reword fn doc), `:257-264` (`seed_relations`), tests module from Task 1

- [ ] **Step 1: Write the failing test**

Append inside the `mod tests` block from Task 1 (after the first test, before the closing `}`):

```rust
    /// Builds a `WatdivGenerator` whose driver paths are never touched;
    /// `seed_relations` reads only `staged`.
    fn stress_generator<'a>(inputs: &'a PipelineInputs<'a>) -> WatdivGenerator<'a> {
        WatdivGenerator {
            inputs,
            workload: Workload::Stress,
        }
    }

    #[test]
    fn stress_workload_seeds_absent_query_predicates() {
        let dir = tempfile::tempdir().unwrap();
        let sparql = write_sparql(
            dir.path(),
            "q.sparql",
            "SELECT ?s ?r WHERE { ?s <http://x/rare> ?r . }\n#end\n",
        );
        let unused = Path::new("unused");
        let inputs = PipelineInputs {
            driver: DriverInputs {
                watdiv_bin: unused,
                vendor_files: unused,
                model_file: unused,
                scale: 1,
                stress: StressParams::default(),
                query_count_per_template: 1,
                use_bwrap: false,
            },
            out_dir: dir.path(),
            bench_name: "unit",
            tag: "unit",
            spec_hash: None,
        };
        let staged = WatdivStaged {
            copied_sparql_paths: vec![sparql],
        };
        let mut part = partitioned_with_title();

        stress_generator(&inputs)
            .seed_relations(&staged, &mut part)
            .unwrap();

        assert!(
            part.predicate_map.contains_key("http://x/rare"),
            "stress workload must seed absent query predicates (issue #63)"
        );
        assert_eq!(part.relations.len(), 2);
        assert!(part.relations[1].tuples.is_empty());
    }
```

- [ ] **Step 2: Run it; it must fail**

```bash
CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf --lib stress_workload_seeds_absent_query_predicates
```
Expected: FAIL with `stress workload must seed absent query predicates (issue #63)`.

If it passes, the gate is not where this plan says; stop and re-read `seed_relations` at `kermit-rdf/src/pipeline.rs:257-264`.

- [ ] **Step 3: Remove the gate and the dead method**

In `kermit-rdf/src/pipeline.rs`, replace the `Workload` block and the seeding fn doc (lines 136-166) with:

```rust
/// Which WatDiv workload `process_artifacts` is finishing. Selects the
/// `meta.json` `kind` discriminator.
pub enum Workload {
    /// Stress workload (`run_pipeline`): watdiv `-s` templates.
    Stress,
    /// Basic Testing workload (`run_basic_pipeline`): static templates.
    Basic,
}

impl Workload {
    /// The `meta.json` `kind` string recorded for this workload.
    fn meta_kind(&self) -> &str {
        match self {
            | Workload::Stress => "watdiv-onthefly",
            | Workload::Basic => "watdiv-basic-onthefly",
        }
    }
}

/// WatDiv draws data (`-d`) and queries (`-s`/`-q`) as independent samples
/// of the model, so any workload can reference a predicate that drew zero
/// triples (rare predicates sit in `<pgroup>` blocks with p < 1 over small
/// entity populations; see issue #63). Seed an empty relation into `part`
/// for each such predicate so translation yields an empty-result join
/// instead of erroring. Mirrors the naming convention used by
/// `partition::partition` for collision-free relation names.
fn seed_missing_predicates(
```

Then replace `seed_relations` (lines 257-264) with:

```rust
    fn seed_relations(
        &self, staged: &WatdivStaged, part: &mut Partitioned,
    ) -> Result<(), RdfError> {
        seed_missing_predicates(part, &staged.copied_sparql_paths)
    }
```

Update the two remaining "basic workload" mentions in the same file:
- Module doc, lines 19-22: change `seeding relations\n//! the basic workload's fixed templates reference` to `seeding empty relations\n//! for query predicates the generated data lacks`.
- `WatdivGenerator` doc, line 203-204: change `plus the workload that decides \`kind\` and seeding` to `plus the workload that decides \`kind\``.

- [ ] **Step 4: Run both unit tests and the crate's test suite**

```bash
CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf --lib pipeline::tests
CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf
```
Expected: both pipeline tests `ok`; the crate suite green. `e2e_watdiv` and `e2e_watdiv_basic` may print `skipping watdiv e2e: ...` outside `nix develop`; that is fine here, Task 4 runs them properly.

- [ ] **Step 5: Clippy the crate**

```bash
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-rdf --all-targets
```
Expected: clean. A `dead_code` warning here means `seeds_missing_predicates` was not deleted.

- [ ] **Step 6: Commit**

```bash
git add kermit-rdf/src/pipeline.rs
git commit -m "fix(rdf): seed empty relations for absent predicates in the stress workload too

WatDiv samples data and queries independently from the model, so a
generated stress query can reference a predicate that drew zero triples
at low scale. The pipeline treated that as a fatal translation error;
e2e_watdiv failed ~13% of isolated runs. Seeding was already the basic
workload's behaviour; the gate rested on a false assumption.

Closes #63"
```

---

### Task 3: Reword the generator hook doc

**Files:**
- Modify: `kermit-rdf/src/generator.rs:178-180`

- [ ] **Step 1: Edit the doc comment**

Replace:

```rust
    /// Hook to add relations the data lacks before Parquet is written (the
    /// WatDiv basic workload seeds empty relations for query predicates
    /// absent from the generated data). Default: nothing.
```

with:

```rust
    /// Hook to add relations the data lacks before Parquet is written (the
    /// WatDiv generator seeds empty relations for query predicates absent
    /// from the probabilistically generated data). Default: nothing.
```

- [ ] **Step 2: Verify docs and clippy (comment-only change, no runtime test needed)**

```bash
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc -p kermit-rdf --no-deps
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy -p kermit-rdf --all-targets
```
Expected: both clean.

- [ ] **Step 3: Commit**

```bash
git add kermit-rdf/src/generator.rs
git commit -m "docs(rdf): describe seed_relations as the WatDiv generator's behaviour"
```

---

### Task 4: Adjust the e2e test and prove seeded relations reach disk

**Files:**
- Modify: `kermit-rdf/tests/e2e_watdiv.rs:101-106` and append a new block after the `expected/` checks (before the final `}` of the test fn)

- [ ] **Step 1: Relax the relation-count assertion**

Replace lines 101-106:

```rust
    let part = kermit_rdf::partition::partition(dir.path().join("raw/data.nt")).unwrap();
    assert_eq!(
        part.relations.len() as u32,
        meta.relation_count,
        "relation count drifted between meta and re-parse"
    );
```

with:

```rust
    // Seeded relations (query predicates that drew zero triples) exist in
    // the output but not in raw/data.nt, so the re-parse is a lower bound.
    let part = kermit_rdf::partition::partition(dir.path().join("raw/data.nt")).unwrap();
    assert!(
        meta.relation_count >= part.relations.len() as u32,
        "meta.relation_count ({}) below re-parsed relation count ({})",
        meta.relation_count,
        part.relations.len()
    );
```

- [ ] **Step 2: Add the YAML-to-Parquet coverage check**

Append inside the test fn, after the `for csv in &csvs { ... }` loop and before the fn's closing `}`:

```rust
    // Every relation the emitted YAML declares must be backed by a Parquet
    // file, including relations seeded empty for absent predicates.
    let yaml = std::fs::read_to_string(dir.path().join("benchmark.yml")).unwrap();
    let def: kermit_bench::BenchmarkDefinition = serde_yaml::from_str(&yaml).unwrap();
    assert!(!def.relations.is_empty(), "benchmark.yml declares no relations");
    for rel in &def.relations {
        let parquet = dir.path().join(format!("{}.parquet", rel.name));
        assert!(
            parquet.exists(),
            "relation {} declared in benchmark.yml has no {}",
            rel.name,
            parquet.display()
        );
    }
```

`kermit_bench` and `serde_yaml` are already dependencies of `kermit-rdf`, so integration tests can use them without a Cargo change.

- [ ] **Step 3: Build the test binary and run the spaced repro**

Inside the dev shell so bwrap and `libstdc++` resolve:

```bash
nix develop --command bash -c '
  CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf --test e2e_watdiv --no-run 2>&1 | tail -2
  BIN=$(ls -t target/debug/deps/e2e_watdiv-* | grep -v "\.d$" | head -1)
  pass=0; fail=0
  for i in $(seq 1 15); do
    if "$BIN" >/tmp/e2e_$i.log 2>&1; then pass=$((pass+1)); else fail=$((fail+1)); echo "run $i FAIL"; tail -5 /tmp/e2e_$i.log; fi
    sleep 1.2
  done
  echo "pass=$pass fail=$fail"'
```
Expected: `pass=15 fail=0`. The `sleep 1.2` is essential: runs inside the same wall-clock second share a WatDiv seed and would not be independent samples. Confirm the log of at least one run does not contain `skipping watdiv e2e`; if all 15 skipped, the environment is wrong and nothing was proven.

Also run the basic e2e once to confirm it still passes:

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo test -p kermit-rdf --test e2e_watdiv_basic
```
Expected: `ok`.

- [ ] **Step 4: Commit**

```bash
git add kermit-rdf/tests/e2e_watdiv.rs
git commit -m "test(rdf): accept seeded relations in e2e_watdiv and check YAML relations reach Parquet"
```

---

### Task 5: Documentation

**Files:**
- Modify: `docs/benchmarks/WATDIV.md:44-55` and the Determinism section
- Modify: `CLAUDE.md:262`

- [ ] **Step 1: Generalise the absent-predicate section**

In `docs/benchmarks/WATDIV.md`, replace the section starting `### Absent predicates in the basic workload` (lines 44-55, through `raise the scale for more meaningful cardinalities.`) with:

```markdown
### Absent predicates

WatDiv draws data (`-d`) and queries (`-s`/`-q`) as independent samples of
the model. Rare predicates live in `<pgroup>` blocks with p < 1 over small
entity populations (for example `MODEL.txt:132`, `0.2 @wsdbm:ProductCategory4`,
over 12–24 products at scale 1), so a predicate can appear in a generated
query while drawing zero triples in the generated data. Measured at scale 1,
some predicate is absent from the whole dataset in roughly 13 % of runs.

Both pipelines therefore seed an **empty relation** for any query-referenced
predicate absent from the generated data (absent predicate = empty relation =
empty result) rather than failing translation. Seeded relations are written
as empty Parquet files and listed in `benchmark.yml` like any other. Expect a
few stress or basic queries at scale 1 to return 0 results for this reason;
at scale 100 and above it is effectively unobservable. The translator still
hard-errors on a predicate that is neither in the data nor seeded, which
after seeding indicates a genuine bug.
```

- [ ] **Step 2: Add the seed-granularity paragraph**

In the `## Determinism` section, directly after the paragraph ending `and different content from the very first line.`, insert:

```markdown
The binary seeds from the wall clock at **one-second granularity**: 200
back-to-back `watdiv -d MODEL.txt 1` runs produced only 20 distinct datasets,
in contiguous byte-identical blocks. Two invocations in the same second are
identical. Consequences: independent samples must be spaced more than one
second apart (`bench gen watdiv --tag a` immediately followed by `--tag b`
yields two differently-tagged, byte-identical datasets); and within one
pipeline run the `-d` and `-q` stages share a seed on an idle machine but
diverge under load, which is why absent-predicate failures once looked
load-correlated.
```

- [ ] **Step 3: One sentence in CLAUDE.md**

In `CLAUDE.md` line 262 (the `WatDiv on-the-fly driver` gotcha), after the sentence ending `so \`expected/*.csv\` is empty for now.`, insert:

```
Both WatDiv workloads seed an empty relation for any query predicate the generated data lacks (`seed_missing_predicates` in `kermit-rdf/src/pipeline.rs`), because `-d` and `-q` are independent draws and rare predicates can miss entirely at scale 1 (issue #63); WatDiv also seeds from the clock at 1-second granularity, so back-to-back generations are byte-identical unless spaced >1 s apart.
```

- [ ] **Step 4: Check the two docs anchors still resolve**

```bash
grep -rn "basic-workload\|#absent-predicates" docs CLAUDE.md
```
Expected: no link references the old `#absent-predicates-in-the-basic-workload` anchor. If one appears, update it to `#absent-predicates`.

- [ ] **Step 5: Commit**

```bash
git add docs/benchmarks/WATDIV.md CLAUDE.md
git commit -m "docs(benchmarks): absent predicates are seeded in both WatDiv workloads; note 1-second seed granularity"
```

---

### Task 6: Full CI gate

**Files:** none new.

- [ ] **Step 1: Format inside the dev shell**

```bash
nix develop --command cargo fmt --all
git diff --stat
```
Expected: either no diff, or only the files touched in Tasks 1-4. If unrelated files change, the flake's nightly has drifted; run `nix flake update rust-overlay` first (see the rustfmt gotcha in `CLAUDE.md`) and do not commit the unrelated rewrites.

- [ ] **Step 2: Workspace checks**

```bash
CARGO_BUILD_JOBS=2 RUSTFLAGS=-Dwarnings cargo clippy --all-targets
CARGO_BUILD_JOBS=2 RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
CARGO_BUILD_JOBS=2 cargo test --workspace
```
Expected: all clean and green.

- [ ] **Step 3: Miri for the crate (inside the dev shell)**

```bash
nix develop --command env CARGO_BUILD_JOBS=2 cargo miri test -p kermit-rdf --lib pipeline::tests
```
Expected: both pipeline unit tests pass under miri (they only touch the filesystem via `tempfile`, which miri with `-Zmiri-disable-isolation` allows).

- [ ] **Step 4: Commit any fmt-only fallout, then review the branch**

```bash
git add -u && git commit -m "style: rustfmt" || true
git log --oneline origin/master..HEAD
```
Expected: the spec commit, plan commit, and the Task 1-5 commits. Landing follows the `project_loom_landing_to_master` memory: fetch, merge `origin/master` in, re-run the gate, push `HEAD:master`. Squashing the test/fix/docs commits into one is optional and only if you are the sole writer on this branch.
