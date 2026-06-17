# kermit-bench

Benchmark definitions, discovery, and caching for the Kermit workspace. Reads YAML benchmark definitions under `benchmarks/`, validates them, and lazily downloads their relation files to a platform cache (`~/.cache/kermit/benchmarks/` on Linux).

## Modules

- [`definition`](src/definition.rs) — Serde types for the YAML schema (`BenchmarkDefinition`, `RelationSource`, `QueryDefinition`, `GeneratorSpec`, `WatdivStressSpec`, `DEFAULT_LUBM_ONTOLOGY`) and [`BenchmarkDefinition::validate`]. `GeneratorSpec::spec_hash` produces the canonical SHA-256 used by drift detection.
- [`discovery`](src/discovery.rs) — loads definitions: `load_benchmark`, `load_all_benchmarks`, `load_all_benchmarks_with_cache`, `load_cached_benchmark`, `list_benchmarks`.
- [`cache`](src/cache.rs) — `ensure_cached`, `is_cached`, `clean_benchmark`, `clean_all`, plus path helpers (`cache_dir`, `relation_cache_path`).
- [`error`](src/error.rs) — the `BenchError` enum, including `SpecDrift` raised by the materialization layer.

## Discovery

`load_all_benchmarks` walks only the workspace `benchmarks/` directory. `load_all_benchmarks_with_cache` additionally walks the platform cache root, picking up generator-produced benchmarks: a cache subdir is consumed only if it contains BOTH `benchmark.yml` AND `meta.json`. The `meta.json` marker is what distinguishes a generator-produced bench from arbitrary cache data; cache entries override workspace entries on name collision.

## YAML schema

See the workspace [`benchmarks/README.md`](../benchmarks/README.md) for the full schema and examples. Each YAML is either *static* (declares `relations` + `queries`) or *generated* (declares `generator`); the two are mutually exclusive (XOR enforced by `BenchmarkDefinition::validate`).

Static form:

```yaml
name: triangle
description: Triangle query over a single edge relation
relations:
  - name: edge
    url: https://example.com/edge.parquet
queries:
  - name: triangle
    description: All triangles in the graph
    query: T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).
```

Generator form:

```yaml
name: watdiv-100
description: WatDiv at scale 100
generator:
  kind: watdiv
  scale: 100
```

## Generator metadata + drift detection

Generator-produced benches carry a `meta.json` (schema_version 2) with a `spec_hash` field. On `bench run <name>`, the binary recomputes the hash from the YAML's `generator:` block and compares it against `meta.json.spec_hash`: equal short-circuits to a cache hit, missing is treated as legacy v1 (drift), and any mismatch yields `BenchError::SpecDrift`. Drift never auto-regenerates — the user must pass `bench run --force <name>` to wipe the cache subdir and rebuild. Enforcement lives in `kermit/src/materialize.rs` (binary side); the data structures and the error variant live here.

## Relationship to other crates

Standalone — `kermit-bench` has no workspace dependencies. Consumed by the [`kermit`](../kermit) CLI binary (drives `bench run`, `bench list`, `bench fetch`, `bench clean`, `bench gen`) and by [`kermit-rdf`](../kermit-rdf), which emits `BenchmarkDefinition` YAMLs into the cache after running its WatDiv/LUBM pipelines.
