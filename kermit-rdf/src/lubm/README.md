# `kermit-rdf::lubm`

On-the-fly Lehigh University Benchmark (LUBM) generation, as a sibling of the
WatDiv pipeline at the crate root.

## Layout

| Module | Responsibility |
|--------|---------------|
| `driver.rs` | Invokes the vendored `lubm-uba.jar` (Java) inside a temp staging dir; gunzips `Universities.nt.gz`; line-filters `<>` document-self triples. |
| `sandbox.rs` | RAII temp dir for one generation. Simpler than the watdiv sandbox — no `bwrap`, no symlink-layout requirements. |
| `entailment.rs` | Univ-Bench TBox forward chainer. Hardcoded rule constants for the axioms relevant to the 14 LUBM queries. |
| `queries.rs` | Static `(name, sparql, expected_cardinality)` triples for Q1–Q14, embedded via `include_str!` from `kermit-rdf/queries/lubm/`. |
| `pipeline.rs` | End-to-end orchestrator: drive → entail → partition → translate → emit YAML (with expected cardinalities)/dict/meta. |

## Output (`~/.cache/kermit/benchmarks/lubm-{scale}-{tag}/`)

```
meta.json                 LubmMeta — kind = "lubm-onthefly"; jar SHA-256, scale, seed, entailment stats
benchmark.yml             kermit BenchmarkDefinition with all 14 queries
dict.parquet              Shared URI/literal → usize dictionary
<predicate>.parquet × N   One per predicate seen in entailed data (e.g. type, worksFor, memberOf, takesCourse, …)
raw/data.nt               Gunzipped jar output, document-self stripped
raw/data.entailed.nt      Post-Univ-Bench-TBox closure; this is what partition reads
raw/queries/q*.sparql     The 14 hand-written LUBM queries verbatim
```

## Entailment rule set (authoritative list)

Codified in `entailment.rs` as Rust constants. Sourced from LUBM paper §2.1
plus the Univ-Bench class hierarchy in `lubm-uba-rs/Ontology.java`. The 14
queries depend on these; missing rules manifest as result counts below the
paper Table 3 reference values.

| Rule kind | Examples | Used by |
|-----------|----------|---------|
| subClassOf transitive closure | GraduateStudent ⊑ Student ⊑ Person; FullProfessor ⊑ Professor ⊑ Faculty ⊑ Employee ⊑ Person; Article ⊑ Publication | Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10, Q13 |
| subPropertyOf duplication | worksFor ⊑ memberOf; headOf ⊑ worksFor; doctoralDegreeFrom ⊑ degreeFrom | Q5, Q12, Q13 |
| owl:TransitiveProperty | subOrganizationOf | Q11 |
| owl:inverseOf | hasAlumnus ↔ degreeFrom | Q13 |
| Realisation | `(?x headOf ?d) ∧ (?d a Department) → (?x a Chair)` | Q12 |

Q1 and Q14 (leaf classes GraduateStudent / UndergraduateStudent, which UBA
asserts directly) and Q2 (no inference) work without any entailment but pass
through the same pipeline for uniformity.

## Determinism

LUBM-UBA's documented invariant is bit-identical output for fixed `(seed, N)`
across thread counts. Because `driver.rs` always passes `--consolidate Maximal`,
the jar routes through `SingleFileConsolidator` and emits exactly one
`Universities.nt.gz` regardless of `--threads`. We still pin `--threads 1` by
default for absolute reproducibility — multi-threaded runs can change triple
ordering *within* that single file even when its byte-level content is
otherwise the same. Override with `--threads N` if you want to stress-test on
multi-core hardware.

The jar SHA-256 is recorded in `meta.json` so a regenerated bench is
distinguishable from a snapshot if anyone rebuilds the jar with a different
JDK or upstream commit.

## Tests

| Test | What it validates | Gate |
|------|------------------|------|
| `lubm/driver.rs::tests` | Missing jar handling; gunzip round-trip | always |
| `lubm/sandbox.rs::tests` | RAII cleanup; staging layout | always |
| `lubm/entailment.rs::tests` | Each rule type on small synthetic input | always |
| `lubm/queries.rs::tests` | All 14 query specs exposed; reference cardinalities match paper | always |
| `tests/e2e_lubm.rs` | Driver runs the real jar end-to-end | `which java` |
| `tests/lubm_entailment_smoke.rs` | Entail real LUBM(1, 0) ABox; closure expands triple count | `which java` |
| `tests/lubm_pipeline.rs` | Full pipeline with placeholder query; meta.json/YAML shape | `which java` |
| `tests/lubm_translator.rs` | All 14 queries translate against entailed predicate map | `which java` |
| `kermit/tests/lubm_cardinalities.rs` | Runs all 14 queries through the join engine on a generated LUBM(1, 0); asserts counts match paper Table 3. Lives in the `kermit` crate — only it depends on both this pipeline and the join engine | `which java` |

## Future work

- **Streaming entailment** for LUBM scales > 5. Current implementation loads
  all triples into a `HashSet` and clones the snapshot once per fixed-point
  iteration. LUBM(1) is comfortable (~250 MB peak); LUBM(5) approaches 1.5 GB;
  LUBM(10) and above can exceed available RAM on developer machines because
  each iteration's snapshot is a full clone of the working set. **Practical
  scale ceiling for the current implementation is LUBM(5).** Streaming or
  delta-based fixpoint would extend this.
- **Vendor-jar SHA-256 verification** in `lubm/driver::drive` — refuse to
  invoke a jar whose hash doesn't match the embedded constant unless
  `--lubm-jar` is explicit.
