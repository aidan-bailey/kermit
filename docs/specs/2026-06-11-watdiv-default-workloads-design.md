# WatDiv default workloads as runnable benchmark suites

- **Date:** 2026-06-11
- **Status:** Accepted
- **Scope:** `kermit-bench`, `kermit-rdf`, `kermit` (CLI), `benchmarks/`, vendored WatDiv assets

## Context and motivation

kermit vendors the WatDiv generator and ships the random **stress** workloads
(`watdiv-stress-100/1000-*`, frozen ZivaHub snapshots) plus a tiny on-the-fly
example (`watdiv-example`). It does **not** ship WatDiv's canonical *default*
query set — the **Basic Testing** templates (Linear `L1–L5`, Star `S1–S7`,
Snowflake `F1–F5`, Complex `C1–C3`; 20 templates) that the literature
standardises on.

Goal: make WatDiv's default workloads available as runnable `bench run`
benchmark suites — **both** the Basic Testing set and the stress workload at
default parameters.

## Decision: generate, do not freeze

WatDiv is **non-deterministic** (no seed; `-d` and `-q` draw fresh randomness
each run). Two models were considered:

- **Frozen snapshots** (the `watdiv-stress-*` pattern): generate once, commit
  data + queries, run repeatedly against the identical artifact. Maximally
  reproducible across machines.
- **Generator-driven** (this design): a declarative `generator:` YAML
  materialised on demand via `kermit-rdf`, cached by `spec_hash`.

We choose **generator-driven**, accepting the non-determinism trade-off
deliberately:

- Within a single comparison session (`bench run … -i all -a all`), the
  workload is materialised **once** and reused from cache across every
  structure/algorithm, so the comparison is internally consistent.
- `spec_hash` drift detection + `--force` give controlled regeneration.
- Regenerable suites are the desired convenience; cross-machine bit-identity is
  explicitly **not** a requirement for this work.

This trade-off is recorded here so the choice is not mistaken for an oversight:
runs are reproducible *within a cached comparison*, not *across cache clears or
machines*. Anyone needing a shareable fixed artifact should fall back to the
frozen-snapshot pattern.

## Out of scope

- Freezing / committing generated data (explicitly rejected above).
- A WatDiv result-cardinality oracle. WatDiv emits no `.desc` sidecars, so
  `expected/*.csv` stays empty and these suites measure time/space only, not
  correctness. (Note: `S1`'s `%v2%` instantiates to a **subject-position
  constant**, so the basic workload *exercises* — but cannot *assert* — the
  join path fixed in the GAO change. A WatDiv oracle is a separate effort.)
- Forwarding `constants_per_query` / `allow_join_vertex` to the binary
  (pre-existing documented limitation). The basic workload sidesteps this
  entirely: each template carries its own `#mapping` placeholder lines.

## Design

### 1. Vendored templates

Vendor the 20 Basic Testing templates to
`kermit-rdf/vendor/watdiv/testsuite/{L1..L5,S1..S7,F1..F5,C1..C3}.txt`
(committed, same upstream provenance as the vendored binary; recorded in
`docs/benchmarks/WATDIV.md`). Each is a standard WatDiv `-q` template: a
`#mapping` line per placeholder plus a BGP `SELECT … WHERE { … }`.

### 2. Schema: new `kind: watdiv-basic` GeneratorSpec variant

Add a `GeneratorSpec::WatdivBasic { scale }` variant (serde tag
`kind: watdiv-basic`) alongside the existing `Watdiv { scale, stress }`.

Rationale: additive and non-breaking — existing `watdiv-*.yml` and the
committed snapshots keep deserialising unchanged. Basic carries **no** stress
parameters (templates are fixed), so a distinct variant models the domain
honestly. Rejected alternatives: a `workload` enum *inside* `Watdiv`
(restructures the existing shape → breaks committed YAMLs); an optional
`workload` field (a "basic" spec would carry ignored `stress` params).

`spec_hash` covers the new variant automatically (it serialises `self`);
`validate()` requires `scale ≥ 1`.

### 3. Pipeline: basic mode

Add a basic path to the `kermit-rdf` driver/pipeline that runs `-d` (data)
then feeds the vendored templates directly to the existing `run_queries`
(`-q`) — **skipping `run_stress` (`-s`)**. All downstream stages
(partition → parquet → `sparql::translator` → `yaml_emit` → `meta`) are reused
unchanged. `meta.json` records the workload kind and template provenance.

### 4. CLI and materialize

- `bench run watdiv-basic` (and any `watdiv-basic`-kind YAML) routes through
  `materialize::materialize`, which dispatches the new variant to the basic
  pipeline.
- `bench gen watdiv --workload basic` for the imperative path (parity with the
  existing `bench gen watdiv`).

### 5. Benchmark YAMLs

- `benchmarks/watdiv-basic.yml` — `generator: { kind: watdiv-basic, scale: 10 }`.
- `benchmarks/watdiv-stress-default.yml` — `generator: { kind: watdiv, scale: 10 }`
  using the default `WatdivStressSpec`, regenerable, complementing the frozen
  `watdiv-stress-100/1000` snapshots.

Default **scale 10** (~1M triples): representative yet practical, and a
one-line change to scale up.

### 6. Testing

A `kermit-rdf` integration test (gated like `e2e_watdiv`: Linux/x86_64, bwrap,
binary loadable) that the basic pipeline emits a `benchmark.yml` whose ~20 BGP
queries translate to valid Datalog. This also gives real-WatDiv coverage of the
subject-position-constant path (`S1`).

## Affected files

- `kermit-rdf/vendor/watdiv/testsuite/*.txt` (new, 20 files)
- `kermit-bench/src/definition.rs` (new `WatdivBasic` variant + validate)
- `kermit-rdf/src/driver/{mod.rs,invoke.rs}`, `kermit-rdf/src/pipeline.rs` (basic mode)
- `kermit/src/materialize.rs`, `kermit/src/main.rs` (dispatch + `--workload`)
- `benchmarks/watdiv-basic.yml`, `benchmarks/watdiv-stress-default.yml` (new)
- `docs/benchmarks/WATDIV.md` (template provenance + workload docs)
- `kermit-rdf/tests/` (basic-pipeline integration test)

## Amendment (2026-06-11): absent-predicate tolerance

During implementation, the basic workload's fixed templates were found to
reference predicates that WatDiv's probabilistic generator omits at a given
scale, which made `translate_query` hard-error (flaky `UnsupportedSparql`).
This amends the "reuse downstream stages unchanged" statement: the basic
pipeline now seeds an empty relation for each query-referenced predicate absent
from the generated data (absent = empty relation = empty result). Implemented
via `translator::bgp_predicate_iris` plus a `seed_missing_predicates` flag on
the WatDiv `process_artifacts` — the stress path passes `false` and is
behaviourally unchanged.
