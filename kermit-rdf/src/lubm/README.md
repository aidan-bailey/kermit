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
| `pipeline.rs` | End-to-end orchestrator: drive → entail → partition → translate → emit YAML/dict/expected/meta. |

## Output (`~/.cache/kermit/benchmarks/lubm-{scale}-{tag}/`)

```
meta.json                 LubmMeta — kind = "lubm-onthefly"; jar SHA-256, scale, seed, entailment stats
benchmark.yml             kermit BenchmarkDefinition with all 14 queries
dict.parquet              Shared URI/literal → usize dictionary
<predicate>.parquet × N   One per predicate seen in entailed data (e.g. type, worksFor, memberOf, takesCourse, …)
raw/data.nt               Gunzipped jar output, document-self stripped
raw/data.entailed.nt      Post-Univ-Bench-TBox closure; this is what partition reads
raw/queries/q*.sparql     The 14 hand-written LUBM queries verbatim
expected/q*.csv           Reference cardinalities (LUBM(1, 0) only; paper Table 3)
```

## Entailment rule set (authoritative list)

Codified in `entailment.rs` as Rust constants. Sourced from LUBM paper §2.1
plus the Univ-Bench class hierarchy in `lubm-uba-rs/Ontology.java`. The 14
queries depend on these; missing rules manifest as result counts below the
paper Table 3 reference values.

| Rule kind | Examples | Used by |
|-----------|----------|---------|
| subClassOf transitive closure | GraduateStudent ⊑ Student ⊑ Person; FullProfessor ⊑ Professor ⊑ Faculty ⊑ Employee ⊑ Person | Q4, Q5, Q6, Q7, Q8, Q9 |
| subPropertyOf duplication | worksFor ⊑ memberOf; headOf ⊑ worksFor; doctoralDegreeFrom ⊑ degreeFrom | Q5, Q12, Q13 |
| owl:TransitiveProperty | subOrganizationOf | Q11 |
| owl:inverseOf | hasAlumnus ↔ degreeFrom | Q13 |
| Realisation | `(?x headOf ?d) ∧ (?d a Department) → (?x a Chair)` | Q12 |

Single-rule queries (Q1, Q3, Q10, Q14) and Q2 (no inference) work without
any entailment but pass through the same pipeline for uniformity.

## Determinism

LUBM-UBA's documented invariant is bit-identical output for fixed `(seed, N)`
across thread counts. We pin `--threads 1` by default for absolute
reproducibility — multi-threaded runs change file emission ordering even when
the byte-level content is the same. Override with `--threads N` if you want to
stress-test on multi-core hardware.

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

## Future work

- **Cardinality regression test** at `tests/lubm_cardinalities.rs`: actually
  run the 14 queries through kermit's join engine on a generated LUBM(1, 0)
  benchmark and assert results match `expected/q*.csv`. This is the load-
  bearing correctness check for the entailment rule set; its absence is the
  main outstanding risk for the LUBM benchmark's thesis-quality status.
- **Streaming entailment** for LUBM scales > 5. Current implementation loads
  all triples into a `HashSet` and clones the snapshot once per fixed-point
  iteration. LUBM(1) is comfortable (~250 MB peak); LUBM(5) approaches 1.5 GB;
  LUBM(10) and above can exceed available RAM on developer machines because
  each iteration's snapshot is a full clone of the working set. **Practical
  scale ceiling for the current implementation is LUBM(5).** Streaming or
  delta-based fixpoint would extend this; not in scope until the cardinality
  regression test is in place.
- **Vendor-jar SHA-256 verification** in `lubm/driver::drive` — refuse to
  invoke a jar whose hash doesn't match the embedded constant unless
  `--lubm-jar` is explicit.
