# kermit-rdf

RDF/SPARQL preprocessing for on-the-fly benchmark generation. Drives the vendored WatDiv binary or LUBM-UBA jar, parses the resulting N-Triples + SPARQL output, and emits a kermit-runnable artifact set: a string-to-`usize` dictionary, per-predicate Parquet relations, a [`BenchmarkDefinition`](../kermit-bench/src/definition.rs) YAML, and (where source cardinalities are available) `expected/<query>.csv` sidecars. The output directory is then consumed by [`kermit-bench`](../kermit-bench) exactly like a hand-authored static benchmark.

Depends on [`kermit-parser`](../kermit-parser), [`kermit-ds`](../kermit-ds), and [`kermit-bench`](../kermit-bench). Consumed by the [`kermit`](../kermit) binary, which dispatches to this crate from `bench gen watdiv|lubm` (imperative) and the declarative `generator:` block in benchmark YAMLs (via `kermit/src/materialize.rs`).

## Pipelines

- [`pipeline::run_pipeline`](src/pipeline.rs) — WatDiv. Inputs: a [`PipelineInputs`](src/pipeline.rs) wrapping the vendored binary path, model file, scale, and stress params. Output: cache subdirectory with `meta.json` (`kind = "watdiv-onthefly"`), `benchmark.yml`, dict, per-predicate Parquet, and the raw `data.nt` / `templates/` / `queries/` carbon copies.
- [`lubm::pipeline::run_lubm_pipeline`](src/lubm/pipeline.rs) — LUBM. Inputs: a [`LubmPipelineInputs`](src/lubm/pipeline.rs) wrapping the vendored jar, scale (universities), seed, and the 14 query specs. Drives the jar, gunzips its output, runs Univ-Bench TBox forward chaining via [`lubm::entailment::entail`](src/lubm/entailment.rs), then partitions the entailed file. Output meta has `kind = "lubm-onthefly"`.

Both record a [`spec_hash`](src/pipeline.rs) into `meta.json` when invoked from a declarative YAML so the materialization layer can detect param drift.

## Shared stages

- [`ntriples`](src/ntriples.rs) — streaming N-Triples parser over `oxttl`, yields `(subject, predicate, object)` with O(1) memory in file size.
- [`partition`](src/partition.rs) — sanitizes predicate URIs and partitions the triple stream into per-predicate `(subject_id, object_id)` relations.
- [`dict`](src/dict.rs) — bidirectional `RdfValue ↔ usize` dictionary, deterministic in insertion order.
- [`parquet`](src/parquet.rs) — Arrow-backed writers for the dictionary and per-predicate relation tables.
- [`sparql::translator`](src/sparql/translator.rs) — translates BGP-only SPARQL `SELECT` queries to Datalog atoms keyed by the partitioned predicate map.
- [`yaml_emit`](src/yaml_emit.rs) — writes the [`BenchmarkDefinition`](../kermit-bench/src/definition.rs) YAML pointing at the emitted Parquet files.
- [`expected`](src/expected.rs) — reads watdiv `.desc` cardinality sidecars and emits `expected/<query>.csv`. The vendored watdiv binary does not emit `.desc` files, so this is effectively a no-op for it; LUBM writes its CSVs directly from `LubmQuerySpec::expected_cardinality`.

## Vendor directories

- [`vendor/lubm-uba/lubm-uba.jar`](vendor/lubm-uba/) — committed (~2.9 MB). Requires a JDK 8 runtime on PATH at generation time. SHA-256 is recorded in `meta.json` for provenance and the jar's regeneration procedure lives at [`vendor/lubm-uba/REGENERATE.md`](vendor/lubm-uba/REGENERATE.md).
- [`vendor/watdiv/`](vendor/watdiv/) — `MODEL.txt`, `files/`, `LICENSE`, and `VERSION` are committed. The binary at `bin/Release/watdiv` is **gitignored** and must be built locally; its SHA-256 is recorded in `meta.json` per generation.

## LUBM queries

The 14 LUBM queries (paper Appendix A) are committed verbatim at [`queries/lubm/q1.sparql … q14.sparql`](queries/lubm/) and embedded into the binary via `include_str!` in [`lubm::queries`](src/lubm/queries.rs). [`lubm::queries::lubm_query_specs`](src/lubm/queries.rs) returns all 14 as [`LubmQuerySpec`](src/lubm/pipeline.rs) values, each carrying its LUBM(1, 0) reference cardinality from paper Table 3 (set `include_expected = false` for any other scale — the cardinalities do not generalise).

## Deeper docs

- [`docs/benchmarks/WATDIV.md`](../docs/benchmarks/WATDIV.md) — user-facing WatDiv usage, stress-template parameters, vendoring rules.
- [`docs/benchmarks/LUBM.md`](../docs/benchmarks/LUBM.md) — user-facing LUBM usage, the 14 queries with reference cardinalities, scale ceiling.
- [`src/lubm/README.md`](src/lubm/README.md) — module-internal contributor doc for the LUBM pipeline (entailment rule set, determinism, test inventory).
- [`vendor/lubm-uba/REGENERATE.md`](vendor/lubm-uba/REGENERATE.md) — jar regeneration procedure.

## Extending

To add a new RDF source type:

1. Add a sibling pipeline module (e.g. `src/foo/pipeline.rs`) that orchestrates driver + entailment (if needed) + partition + translate + emit, reusing the shared stages above.
2. Add a `kind: foo` variant to [`GeneratorSpec`](../kermit-bench/src/definition.rs) and wire it into the materialize dispatch in [`kermit/src/materialize.rs`](../kermit/src/materialize.rs).
