# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-06-11

### Added

- WatDiv "Basic Testing" pipeline: `run_basic_pipeline` and `drive_basic`, with vendored L/S/F/C query templates
- LUBM cardinality regression test and a WatDiv Basic Testing end-to-end test

### Changed

- Parameterize `process_artifacts` over the meta kind
- Split the LUBM entailment loop into per-rule functions; lift `sha256_file` to the crate root; name parquet column strings as constants; extract shared timestamp helpers
- `kermit-ds` dependency to 0.2.0
- `kermit-bench` dependency to 0.1.2

### Fixed

- Seed empty relations for predicates absent from the basic-workload data
- Tighten subprocess and path error handling
- `owl:inverseOf` entailment rule now skips blank-node objects

## [0.1.0] - 2026-05-05

### Added

- Initial release of `kermit-rdf`: RDF/SPARQL preprocessing pipelines for on-the-fly Kermit benchmark generation.
- WatDiv pipeline (`pipeline::run_pipeline`) driving the vendored `watdiv` binary through generation, partitioning, and YAML emission.
- LUBM pipeline (`lubm::pipeline::run_lubm_pipeline`) driving the vendored `lubm-uba.jar`, including N-Triples extraction and Univ-Bench TBox forward chaining.
- Shared pipeline stages: `partition`, `parquet`, `dict`, `sparql::translator`, `yaml_emit`, `expected`.
- Univ-Bench entailment module (`lubm::entailment`) implementing subClassOf / subPropertyOf / `owl:TransitiveProperty` / `owl:inverseOf` rules and `Chair` realisation from `headOf`.
- 14 LUBM queries committed at `queries/lubm/q*.sparql` (paper Appendix A) with reference cardinalities for LUBM(1, 0), exposed via `lubm::queries::lubm_query_specs`.
- Vendored `lubm-uba.jar` (committed, ~2.9 MB) at `vendor/lubm-uba/`; vendored WatDiv source tree at `vendor/watdiv/` (binary gitignored, build locally).
- N-Triples streaming parser, SPARQL→Datalog translator built on `spargebra`, dictionary encoder, and Parquet writers per predicate.
- End-to-end integration tests for both pipelines (auto-skip on non-Linux/non-x86_64 hosts and where bwrap cannot construct the dict-words bind).
