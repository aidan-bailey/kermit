# LUBM (Lehigh University Benchmark)

The Lehigh University Benchmark is a Semantic Web knowledge-base benchmark
introduced by Guo, Pan, and Heflin (2005). It exercises an OWL-Lite ontology
of moderate complexity with synthetic, scalable instance data and 14
hand-designed queries. Kermit uses LUBM as a counterpart to WatDiv: where
WatDiv stresses the engine with thousands of mechanically-generated query
shapes, LUBM provides 14 queries that can be named, reasoned about, and
discussed individually — including two clean triangle joins (Q2, Q9) that are
the canonical workload Veldhuizen 2014 designed Leapfrog Triejoin to dominate.

For a side-by-side comparison with other benchmarks in the suite, see
[`README.md`](README.md#cross-benchmark-comparison).

## How to run

### On-the-fly generation

```bash
kermit bench gen lubm --scale N --tag STR \
    [--seed N=0] [--start-index N=0] \
    [--threads N=1] \
    [--lubm-jar PATH | env KERMIT_LUBM_JAR] \
    [--ontology URL] \
    [--output-dir PATH]
```

Produces `~/.cache/kermit/benchmarks/lubm-{scale}-{tag}/` runnable via the
unmodified `kermit bench run`. The bench name is workspace-collision-checked
against committed YAMLs at runtime.

| Flag | Default | Notes |
|------|---------|-------|
| `--scale` | required | Universities to generate (`-u`); ≥ 1 |
| `--tag` | required | Suffix on the bench name; pick a value that won't collide with committed snapshots |
| `--seed` | `0` | LUBM-UBA's documented default; deterministic per `(scale, seed)` |
| `--start-index` | `0` | Starting university number |
| `--threads` | `1` | Single-threaded by default for byte-level reproducibility |
| `--lubm-jar` | vendored | Override path to `lubm-uba.jar`; also reads `KERMIT_LUBM_JAR` |
| `--ontology` | canonical Univ-Bench URL | Base IRI for generated entity URIs |
| `--output-dir` | `~/.cache/kermit/benchmarks` | Custom output dirs are NOT auto-discovered by `bench list/fetch/run` |

End-to-end runtime for LUBM(1, 0) is ~5 s (jar generation ~0.9 s, entailment
~3 s, partition + parquet emission ~1 s).

### Declarative YAML spec (commit-and-run)

Alternatively, commit a `benchmarks/<name>.yml` declaring a `generator:`
block. `bench run <name>` materialises the data on first invocation:

```yaml
# benchmarks/lubm-1.yml
name: lubm-1
description: "LUBM(1, 0)"
generator:
  kind: lubm
  scale: 1
```

Optional fields: `seed`, `threads`, `start_index`, `ontology`, and
`queries: [q1, q3, q5]` to run a subset of the 14 queries (validation
errors if any name is outside `q1..q14`). Editing the YAML's params and
re-running errors with `SpecDrift` — pass `--force` to opt into
regenerating. See `benchmarks/README.md` for the full schema.

## Pipeline

```
lubm-uba.jar (vendored)        kermit-rdf::lubm::driver
        │                              │
        │ -f NTRIPLES                  │ gunzip
        │ --consolidate Maximal        │ strip <> document-self triples
        │ --compress                   ▼
        ▼                       raw/data.nt
   Universities.nt.gz                  │
                                       │ kermit-rdf::lubm::entailment
                                       │ (Univ-Bench TBox forward chain)
                                       ▼
                                raw/data.entailed.nt
                                       │
                                       │ kermit-rdf::partition
                                       ▼
                            <pred>.parquet × N + dict.parquet
                                       │
                                       │ kermit-rdf::sparql::translator
                                       │ (14 LUBM SPARQL → Datalog rules)
                                       ▼
                              benchmark.yml + meta.json + expected/*.csv
```

## Output layout

```
~/.cache/kermit/benchmarks/lubm-{scale}-{tag}/
  meta.json                    LubmMeta — kind = "lubm-onthefly"
  benchmark.yml                kermit BenchmarkDefinition with all 14 queries
  dict.parquet                 Shared URI/literal → usize dictionary
  <predicate>.parquet × N      One per predicate seen in entailed data
  raw/data.nt                  Gunzipped jar output, document-self stripped
  raw/data.entailed.nt         Post-Univ-Bench-TBox closure; what partition reads
  raw/queries/q1.sparql … q14.sparql
  expected/q1.csv … q14.csv    Reference cardinalities (LUBM(1, 0) only — see below)
```

`meta.json` records the SHA-256 of the jar that produced the snapshot, the
ontology IRI, ISO timestamp, pre/post entailment triple counts, and
fixed-point iteration count.

## Inference / preprocessing

Hardcoded constants in `kermit-rdf/src/lubm/entailment.rs`. Sourced from the
LUBM paper §2.1 plus the Univ-Bench class hierarchy in
`lubm-uba-rs/Ontology.java`. Not a general OWL reasoner.

| Rule kind | Examples | Required by |
|-----------|----------|-------------|
| subClassOf transitive closure | GraduateStudent ⊑ Student ⊑ Person; FullProfessor ⊑ Professor ⊑ Faculty ⊑ Employee ⊑ Person | Q4, Q5, Q6, Q7, Q8, Q9 |
| subPropertyOf duplication | worksFor ⊑ memberOf; headOf ⊑ worksFor; doctoralDegreeFrom ⊑ degreeFrom | Q5, Q12, Q13 |
| owl:TransitiveProperty | subOrganizationOf | Q11 |
| owl:inverseOf | hasAlumnus ↔ degreeFrom | Q13 |
| Realisation | `(?x headOf ?d) ∧ (?d a Department) → (?x a Chair)` | Q12 |

Single-rule queries (Q1, Q3, Q10, Q14) and Q2 (no inference) require no
entailment but pass through the same pipeline for uniformity.

The entailment loop has a hard cap of 64 fixed-point iterations and errors
out if convergence is not reached — a buggy rule that re-triggers itself must
not hang silently. LUBM(1, 0) converges in 3 iterations; expansion factor is
~26 % (103 074 input → 127 974 output triples).

## Workload reference

Lifted verbatim from the LUBM paper Appendix A (pp. 175–177). Committed at
`kermit-rdf/queries/lubm/q*.sparql` and embedded via `include_str!` into
`kermit-rdf::lubm::queries`. URIs reference `Department0.University0.edu`,
which exists in any LUBM(N ≥ 1) by the data generator's deterministic
0-indexed naming, so queries are universal across all scales.

| # | Shape | Pre-materialisation needed | LUBM(1, 0) cardinality |
|---|-------|---------------------------|------------------------|
| Q1 | `GraduateStudent` who `takesCourse` GraduateCourse0 | none | 4 |
| Q2 | **Triangle**: GraduateStudent / University / Department joined by memberOf, subOrgOf, undergraduateDegreeFrom | subClassOf closures only | 0 |
| Q3 | `Publication` of AssistantProfessor0 | subClassOf (Publication hierarchy) | 6 |
| Q4 | Star: `Professor` with name/email/phone, worksFor Department0 | subClassOf (Professor hierarchy) | 34 |
| Q5 | `Person` memberOf Department0 | subClassOf (Person) + subPropertyOf (worksFor → memberOf) | 719 |
| Q6 | All `Student` | subClassOf (Student) | 7 790 |
| Q7 | Student takesCourse Y, AssociateProfessor0 teacherOf Y | subClassOf (Student) | 67 |
| Q8 | Student/Department/email scoped to University0 | subClassOf (Student) | 7 790 |
| Q9 | **Triangle**: Student/Faculty/Course via advisor, teacherOf, takesCourse | subClassOf (Student, Faculty) | 208 |
| Q10 | Student takesCourse GraduateCourse0 | none — explicit subClassOf only | 4 |
| Q11 | ResearchGroup subOrganizationOf+ University0 | **transitive closure** of `subOrganizationOf` | 224 |
| Q12 | Chair / Department / worksFor / subOrgOf | **realisation**: Chair derived from headOf | 15 |
| Q13 | Person hasAlumnus University0 | **inverseOf + subPropertyOf** | 1 |
| Q14 | All `UndergraduateStudent` | none | 5 916 |

Reference cardinalities are from the LUBM paper Table 3 (DLDB-OWL column —
the only system in the paper that achieved 100 % completeness across all
queries) and are written to `expected/q*.csv` only when `--scale 1`. At other
scales the queries still run but expected files are omitted to avoid
misleading the cardinality test.

## Determinism

LUBM-UBA's documented invariant is bit-identical output for fixed
`(seed, scale)` across thread counts. We pin `--threads 1` by default for
absolute reproducibility — multi-threaded runs change file emission ordering
even when byte-level content is the same. Override with `--threads N` for
multi-core throughput when reproducibility is not required.

The vendored jar's SHA-256 is recorded in `meta.json` so a regenerated bench
is distinguishable post-hoc if the jar is rebuilt against a different JDK or
upstream commit. There is no runtime hash check that *rejects* a mismatched
jar (listed as future work).

## Practical scale ceiling

The current entailment implementation is **in-memory**: the entire ABox is
loaded into a `HashSet`, and the working set is cloned once per fixed-point
iteration. Scale guidance:

| Scale | Triples (post-entailment) | Peak memory | Notes |
|-------|---------------------------|-------------|-------|
| LUBM(1, 0) | ~128 K | ~250 MB | Comfortable; smoke-tested |
| LUBM(5, 0) | ~640 K | ~1.5 GB | Approaching practical ceiling |
| LUBM(10, 0) | ~1.3 M | ~3 GB+ | May exceed dev RAM; not recommended |
| LUBM(50, 0) | ~6.9 M | ~15 GB+ | Will OOM on most machines |

Streaming or delta-based fixed-point is future work (see below).

## Vendored generator

`kermit-rdf/vendor/lubm-uba/lubm-uba.jar` (~2.9 MB, committed). Provenance:

| Field | Value |
|-------|-------|
| Source | <https://github.com/rvesse/lubm-uba> at branch `improved` |
| Commit | `32f83e3b8d88550af77fa563e94039ebf4229d16` |
| Local path | `/tb/Source/Academia/lubm-uba-rs` |
| JDK | OpenJDK 1.8.0_472 (NixOS) |
| Maven | Apache Maven 3.9.12 |
| Source/target level | Java 1.7 (per upstream `pom.xml`) |

Rebuild instructions: `kermit-rdf/vendor/lubm-uba/REGENERATE.md`. The upstream
fork preserves bit-identical output relative to the original SWAT Lab UBA
generator; do **not** modernise Java source level when rebuilding.

## Tests

| Test | What it validates | Gate |
|------|-------------------|------|
| `kermit-rdf/tests/e2e_lubm.rs` | Vendored jar drives end-to-end and produces a non-empty `Universities.nt` matching the canonical LUBM(1, 0) triple count | `java` on PATH; not miri |
| `kermit-rdf/tests/lubm_entailment_smoke.rs` | Univ-Bench TBox closure expands triple count and preserves all original triples on a real LUBM(1, 0) ABox | `java` on PATH; not miri |
| `kermit-rdf/tests/lubm_translator.rs` | All 14 LUBM SPARQL queries translate to valid Datalog rules against the partitioned entailed predicate map | `java` on PATH; not miri |
| `kermit-rdf/tests/lubm_pipeline.rs` | Full driver → entail → partition → translate → emit pipeline; on-disk output layout, `meta.json` shape, `benchmark.yml` validity | `java` on PATH; not miri |
| `kermit/tests/lubm_cardinalities.rs` | Generates LUBM(1, 0) end-to-end, runs all 14 queries through the join engine, asserts each result count matches the paper Table 3 reference | `java` on PATH; not miri |

The cardinality regression test lives in the `kermit` crate (not `kermit-rdf`)
because only the binary crate depends on both the LUBM pipeline and the join
engine (`kermit-algos` + `DatabaseEngine`), mirroring `watdiv_correctness.rs`.
It is the load-bearing correctness check for the entailment rule set and the
const-view-rewrite join path; finding it green is the precondition for relying
on Q5–Q13 results from this pipeline.

## References

- Y. Guo, Z. Pan, J. Heflin. *LUBM: A benchmark for OWL knowledge base
  systems.* Web Semantics 3 (2005) 158–182.
- T. L. Veldhuizen. *Leapfrog Triejoin: a worst-case optimal join algorithm.*
  ICDT 2014.
- Univ-Bench TBox: <http://www.lehigh.edu/~zhp2/2004/0401/univ-bench.owl>
- Upstream UBA generator (Vesse fork): <https://github.com/rvesse/lubm-uba>
- Module README: `kermit-rdf/src/lubm/README.md`
- Rebuild instructions: `kermit-rdf/vendor/lubm-uba/REGENERATE.md`
- Sibling benchmark reference: [`WATDIV.md`](WATDIV.md)

## Future work

- **Streaming entailment** for LUBM scales > 5.
- **Vendor-jar SHA-256 verification** in `lubm/driver::drive` — refuse to
  invoke a jar whose hash doesn't match the embedded constant unless
  `--lubm-jar` is explicit.
- **Soundness/completeness metrics** from the LUBM paper §2.4.4 (currently
  out of scope; cardinality match is sufficient for thesis purposes).
