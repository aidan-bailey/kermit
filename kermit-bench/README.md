# kermit-bench

Benchmark definitions, discovery, and caching for the Kermit workspace. Reads YAML benchmark definitions under `benchmarks/`, validates them, and lazily downloads their relation files to a platform cache (`~/.cache/kermit/benchmarks/` on Linux).

## Modules

- [`definition`](src/definition.rs) — Serde types for the YAML schema (`BenchmarkDefinition`, `RelationSource`, `QueryDefinition`) and [`BenchmarkDefinition::validate`].
- [`discovery`](src/discovery.rs) — loads definitions from a workspace root: `load_benchmark`, `load_all_benchmarks`, `list_benchmarks`.
- [`cache`](src/cache.rs) — `ensure_cached`, `is_cached`, `clean_benchmark`, `clean_all`, plus path helpers.
- [`error`](src/error.rs) — the `BenchError` enum used throughout.

## YAML schema

See the workspace [`benchmarks/README.md`](../benchmarks/README.md) for the full schema and examples. In brief:

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

## Relationship to other crates

Standalone — `kermit-bench` has no workspace dependencies. The CLI binary in [`kermit`](../kermit) consumes it to drive `bench run`, `bench list`, `bench fetch`, and `bench clean`.
