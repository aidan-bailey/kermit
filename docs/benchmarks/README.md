# Benchmark reference

This directory contains reference docs for Kermit's generator-driven RDF
benchmarks. For each benchmark, see its dedicated doc; for cross-benchmark
comparison, see the matrix below. The schema at the bottom of this file is
the canonical template every benchmark doc follows.

## Index

- [LUBM](LUBM.md) — 14 hand-designed queries over a synthetic OWL-Lite
  university ontology, including two clean triangle joins (Q2, Q9). The
  named-query workload Veldhuizen 2014 designed Leapfrog Triejoin to
  dominate.
- [WatDiv](WATDIV.md) — ~12 400 mechanically-generated stress queries per
  workload spanning star, snowflake, and chain shapes. The distributional
  counterpart to LUBM.

## Cross-benchmark comparison

Rows cluster by purpose: workload-shape rows describe what the workload
looks like; verification rows describe how a run is checked for correctness;
operational rows describe how it actually runs.

| Dimension | LUBM | WatDiv |
|---|---|---|
| Query count per dataset | 14 | 12 400 across 12 stress files |
| Authoring | Hand-designed for specific OWL features | Mechanically generated from templates |
| Predicate arity | All binary + unary type lookups | All binary |
| Inference required | OWL-Lite (subClassOf, subPropertyOf, transitivity, inverseOf, realisation) | None — data is pre-materialised |
| Triangle queries | **Q2, Q9 — explicit hand-designed** | Incidental (e.g. q0010 sharing a country variable) |
| Self-joins | None | Yes (e.g. `friendof(V2, V2)`) |
| Result oracle | Paper Table 3, manually transcribed | None — vendored binary emits no `.desc` |
| Reproducibility | Deterministic per `(seed, scale)` | Non-deterministic; tag-based snapshots |
| Sandbox | — | bwrap required (or `--no-bwrap`) |
| Vendored asset | Committed (~2.9 MB jar) | Gitignored; build locally |
| Pipeline command | `kermit bench gen lubm` | `kermit bench gen watdiv` |

Cell conventions: `—` for "inapplicable"; `None` when the value is genuinely
zero/empty (e.g. LUBM has no self-joins by design); bold marks a row's most
distinguishing value.

## Schema for benchmark docs

Each benchmark doc in this directory follows the schema below. The schema
serves two purposes: it makes the docs uniform enough to skim quickly, and
it gives contributors a checklist when adding a new generator-driven RDF
benchmark.

### Required sections (in canonical order)

| # | Heading | Content |
|---|---|---|
| 1 | `# <NAME> (<expansion>)` | 1–2 paragraphs (~80–150 words): what it is, who introduced it (paper cite), why kermit uses it. Closing sentence positions it vs. siblings and links to the comparison matrix above. |
| 2 | `## How to run` | 1–3 `###` subsections: **Committed snapshots** (when applicable), **On-the-fly generation** (always), **Declarative YAML spec (commit-and-run)** (always). On-the-fly subsection contains a `Flag \| Default \| Notes` CLI flag table. YAML subsection contains a code block showing every `generator:` key with required/optional notes below. |
| 3 | `## Pipeline` | ASCII diagram: generator on the left, kermit-rdf stages on the right, arrows down. 1–2 sentences flagging pipeline-specific quirks. |
| 4 | `## Output layout` | Code block with the directory tree under `~/.cache/kermit/benchmarks/<bench-id>/` and one-line annotations. Short paragraph describing `meta.json` contents. |
| 5 | `## Workload reference` | Named-query benchmarks: `# \| Shape \| Pre-materialisation needed \| Reference cardinality` table (cite source). Mechanical workloads: `Parameter \| Effect` table for workload-shape params + distribution stats. Always include a representative emitted query so the reader sees what LFTJ consumes. |
| 6 | `## Determinism` | Whether the benchmark is deterministic and per what tuple of inputs. How reproducibility is achieved (or why it can't be). What `meta.json` records for post-hoc identification. |
| 7 | `## Vendored generator` | `Field \| Value` table: source URL, commit/tag, build toolchain, language level if relevant, vendor path, committed-vs-gitignored. Short paragraph on regeneration; link to `REGENERATE.md` if any. |
| 8 | `## Tests` | `Test \| What it validates \| Gate` table. Gate column lists platform/tooling preconditions (e.g. "`java` on PATH; not miri"). |
| 9 | `## References` | Bullet list. Required entries: original paper(s); upstream generator URL; sibling-benchmark doc link; pointers to relevant `kermit-rdf` module READMEs and `REGENERATE.md` files. Format: hanging-indent for academic cites, angle-bracketed bare URLs for upstream pointers, backticked relative paths for in-repo references. |
| 10 | `## Future work` | Bullet list. Each bullet: bold lead phrase, then 1–2 sentences explaining the gap and (if known) the load-bearing risk. |

### Applicable-when-relevant sections

Each has a fixed slot position — the section appears between the listed
required sections, never elsewhere. Skip the heading entirely when the
trigger doesn't apply (do not write "N/A").

| Heading | Slot | Trigger | Content |
|---|---|---|---|
| `## Inference / preprocessing` | Between §4 and §5 | Pipeline has a non-trivial step beyond partition + parquet (e.g. forward chaining, materialisation). | `Rule kind \| Examples \| Required by` table; convergence/termination notes; expansion factor at a representative scale. |
| `## Practical scale ceiling` | Between §6 and §7 | Memory or runtime is a meaningful constraint at thesis-relevant scales. | `Scale \| Output size \| Peak memory \| Notes` table; short paragraph naming the bottleneck with a forward-link to Future work. |
| `## Sandboxing` | Between §7 and §8 | Generator requires sandboxing or unusual host setup. | Short paragraph naming the requirement; bulleted list of bind-mount / setup steps the driver performs; how to disable; host-environment notes. |

### Heading conventions

- Heading text is **exact** at the `##` level. Descriptive `###` subheadings
  inside a section are free-form (e.g. `### The 14 queries` inside
  `## Workload reference`).
- Cell-level rules: `—` (em dash) for "inapplicable"; `None` for "the value
  is genuinely zero/empty"; bold marks a row's distinguishing value when
  meaningful.
- Table column headers across docs:
  - `Field | Value` for provenance / metadata tables.
  - `Flag | Default | Notes` for CLI flag tables.
  - `Parameter | Effect` for semantic parameter tables.
  - `Test | What it validates | Gate` for the Tests section.

### Parameter classes

Parameters appear in three places. Never duplicate without a cross-reference.

1. **CLI flags** (e.g. `--scale`, `--seed`) — in §2 "How to run / On-the-fly
   generation" as a `Flag | Default | Notes` table.
2. **Declarative YAML keys** (e.g. `generator.scale`) — in §2 "How to run /
   Declarative YAML spec" as a YAML code block followed by a required/optional
   list and any validation rules.
3. **Workload-shape parameters** (e.g. WatDiv stress params) — in §5
   "Workload reference" as `Parameter | Effect`. When these are also settable
   via class 1 (CLI flag) or class 2 (YAML key), include a one-line
   cross-reference from the Workload reference table back to the relevant
   subsection of How to run.

### Adding a new benchmark

1. Add a row to the [comparison matrix](#cross-benchmark-comparison) above.
   Adding new dimensions (rows) requires filling them in for *every* existing
   benchmark in the same change.
2. Add an entry to the [Index](#index) above.
3. Create `docs/benchmarks/<NAME>.md` following the schema. Required sections
   in canonical order; applicable-when-relevant sections only when their
   triggers apply. Use the existing LUBM and WatDiv docs as worked examples.
