# Kermit Benchmarks

This directory holds YAML benchmark definitions consumed by the `kermit bench` CLI and the `kermit-bench` crate. Each `*.yml` file declares a benchmark: a set of relations (downloaded from remote URLs and cached locally) and one or more named Datalog queries to run against them.

## File layout

- One benchmark per `*.yml` file. The filename stem must match the `name` field exactly (enforced by `kermit_bench::discovery::load_benchmark`).
- `triangle.yml` is the minimal working example.
- `oxford-uniform-s*.yml` and `oxford-zipf-s*.yml` are realistic multi-relation benchmarks from the Oxford DSI dataset.

## Schema

Each file deserializes into `BenchmarkDefinition` (see `kermit-bench/src/definition.rs`).

| Field                       | Type               | Required | Description |
|-----------------------------|--------------------|----------|-------------|
| `name`                      | string             | yes      | Benchmark identifier. Must match the filename stem and be non-empty. |
| `description`               | string             | yes      | Human-readable summary. May be empty but the key must be present. |
| `relations`                 | list               | yes      | Relations to load. Must be non-empty; names must be unique within the file. |
| `relations[].name`          | string             | yes      | Relation identifier referenced in Datalog queries below. |
| `relations[].url`           | string             | yes      | HTTPS download URL for the relation's Parquet file. |
| `queries`                   | list               | yes      | Named queries to run. Must be non-empty; names must be unique. |
| `queries[].name`            | string             | yes      | Query identifier (used by `kermit bench run <benchmark> -q <query>`). |
| `queries[].description`     | string             | yes      | Human-readable summary of what the query computes. |
| `queries[].query`           | string             | yes      | A Datalog rule parsed by `kermit-parser`. See the grammar below. |

Structural invariants are checked at load time by `BenchmarkDefinition::validate`.

## Datalog query syntax

Queries follow the Datalog rule syntax accepted by `kermit-parser`:

```
Head(Var1, Var2, …) :- Body1(…), Body2(…), …, BodyN(…).
```

- Variables are upper-case identifiers (`X`, `Var1`).
- Atoms are lower-case or numeric literals (match the `Term::Atom` variant).
- `_` is a placeholder for an unused position.
- A query is terminated with a period.
- Body predicate names must match a `relations[].name` declared above.

## Minimal example (`triangle.yml`)

```yaml
name: triangle
description: "Triangle query over edge relation"
relations:
  - name: edge
    url: "https://zivahub.uct.ac.za/ndownloader/files/PLACEHOLDER"
queries:
  - name: triangle
    description: "Three-way cyclic join"
    query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
```

## Multi-relation, multi-query example

See `oxford-uniform-s1.yml` for a benchmark with 8 relations and 3 queries (`binary-join`, `triangle`, `six-way`). Each query shares the same relation set, so downloading the data once amortises across all queries.

## Caching

Relation files are downloaded on first use and cached at:

- Linux: `~/.cache/kermit/benchmarks/<benchmark-name>/<relation-name>.parquet`
- macOS: `~/Library/Caches/kermit/benchmarks/<benchmark-name>/<relation-name>.parquet`
- Windows: `%LOCALAPPDATA%\kermit\benchmarks\<benchmark-name>\<relation-name>.parquet`

Exact layout comes from `dirs::cache_dir()` (see `kermit-bench/src/cache.rs`). Use `kermit bench clean <name>` to drop a single benchmark's cache, or `kermit bench clean --all` to purge everything.

Downloads write to a `*.parquet.part` file and atomically rename on completion, so an interrupted download cannot leave a partial file that later reads trust.

## Adding a new benchmark

1. Create `benchmarks/<name>.yml` with the schema above.
2. Run `cargo run -- bench list` to confirm discovery.
3. Run `cargo run -- bench fetch <name>` to pre-download the relations.
4. Run `cargo run -- bench run <name>` to execute the benchmark.

Because `load_all_benchmarks` sorts entries by filename, benchmarks surface in alphabetical order in CLI listings.
