# Kermit Benchmarks

This directory holds YAML benchmark definitions consumed by the `kermit bench` CLI and the `kermit-bench` crate. Each `*.yml` file declares a benchmark: a set of relations (downloaded from remote URLs and cached locally) and one or more named Datalog queries to run against them.

## File layout

- One benchmark per `*.yml` file. The filename stem must match the `name` field exactly (enforced by `kermit_bench::discovery::load_benchmark`).
- `triangle.yml` is the minimal working example.
- `oxford-uniform-s*.yml` and `oxford-zipf-s*.yml` are realistic multi-relation benchmarks from the Oxford DSI dataset.
- `watdiv-example.yml`, `watdiv-basic.yml`, `lubm-example.yml`, and `lubm-reference.yml` are generator-driven examples illustrating the generator schema below; `watdiv-stress-default.yml` is a declarative generator spec (distinct from the frozen `watdiv-stress-*` snapshots covered later).

## Schema

Each file deserializes into `BenchmarkDefinition` (see `kermit-bench/src/definition.rs`).

A benchmark is **either** static (the relations and queries are spelled out directly) **or** generator-driven (a `generator:` block declares parameters and the data is produced on demand by a `kermit-rdf` pipeline). The two are mutually exclusive — `BenchmarkDefinition::validate` enforces the XOR.

### Common fields

| Field                       | Type               | Required | Description |
|-----------------------------|--------------------|----------|-------------|
| `name`                      | string             | yes      | Benchmark identifier. Must match the filename stem and be non-empty. |
| `description`               | string             | yes      | Human-readable summary. May be empty but the key must be present. |

### Static benchmark fields

| Field                       | Type               | Required | Description |
|-----------------------------|--------------------|----------|-------------|
| `relations`                 | list               | yes      | Relations to load. Must be non-empty; names must be unique within the file. |
| `relations[].name`          | string             | yes      | Relation identifier referenced in Datalog queries below. |
| `relations[].url`           | string             | one of   | HTTP(S) download URL for the relation's Parquet file. Fetched once into the cache. |
| `relations[].path`          | string             | one of   | Workspace-relative path to a committed CSV or Parquet file. Read in place — never downloaded, copied, or cached. |

Exactly one of `url` and `path` must be set per relation. Use `url` for large or
externally-hosted datasets; use `path` for small worked examples that should be
readable in the repository and runnable offline on a cold cache.

Two constraints apply to `path`, both enforced by `BenchmarkDefinition::validate`:

- It must stay inside the workspace — no absolute paths, no `..` components — because
  it is resolved by joining onto the workspace root.
- Its **file stem must equal the relation's `name`**. The CSV and Parquet loaders take
  the relation's name from the filename, so `path: data/edges.csv` under
  `name: edge` would load a relation the queries cannot refer to.
| `queries`                   | list               | yes      | Named queries to run. Must be non-empty; names must be unique. |
| `queries[].name`            | string             | yes      | Query identifier (used by `kermit bench run <benchmark> -q <query>`). |
| `queries[].description`     | string             | yes      | Human-readable summary of what the query computes. |
| `queries[].query`           | string             | yes      | A Datalog rule parsed by `kermit-parser`. See the grammar below. |

### Generator block (declarative)

A `generator:` block replaces `relations` and `queries`. On `bench run <name>`, the data is materialised on first invocation and cached for subsequent runs.

| Field                       | Type               | Required | Description |
|-----------------------------|--------------------|----------|-------------|
| `generator.kind`            | `watdiv` \| `watdiv-basic` \| `lubm` | yes      | Selects the underlying `kermit-rdf` pipeline. |
| `generator.scale`           | u32 (≥ 1)          | yes      | Scale factor (`watdiv -d <N>` or `lubm-uba -u <N>`). |

WatDiv-specific fields (under `generator:` when `kind: watdiv`):

| Field                              | Type   | Default | Description |
|------------------------------------|--------|---------|-------------|
| `generator.stress.max_query_size`  | u32    | 5       | `<max-query-size>` in stress templates. |
| `generator.stress.query_count`     | u32    | 20      | `<query-count>` per stress template. |
| `generator.stress.constants_per_query` | u32 | 2     | `<constants-per-query>`. |
| `generator.stress.allow_join_vertex` | bool | false   | `<allow-join-vertex>`. |

When `kind: watdiv-basic`, the generator runs the WatDiv Basic Testing workload — the 20 canonical L/S/F/C query templates (`kermit_rdf::pipeline::run_basic_pipeline`). The templates are fixed, so there is no `stress` block; `scale` is the only field. See `benchmarks/watdiv-basic.yml`.

LUBM-specific fields (under `generator:` when `kind: lubm`):

| Field                       | Type        | Default                                                     | Description |
|-----------------------------|-------------|-------------------------------------------------------------|-------------|
| `generator.seed`            | u32         | 0                                                           | RNG seed. |
| `generator.threads`         | u32         | 1                                                           | Worker thread count. |
| `generator.start_index`     | u32         | 0                                                           | Starting university index. |
| `generator.ontology`        | string      | `http://www.lehigh.edu/~zhp2/2004/0401/univ-bench.owl`     | Univ-Bench TBox IRI. |
| `generator.queries`         | list of `q1`..`q14` | all 14 if omitted                                  | Subset of LUBM queries to materialise. |

Structural invariants are checked at load time by `BenchmarkDefinition::validate`.

## Datalog query syntax

Queries follow the Datalog rule syntax accepted by `kermit-parser`:

```
Head(Var1, Var2, …) :- Body1(…), Body2(…), …, BodyN(…).
```

- Variables are upper-case identifiers (`X`, `Var1`).
- Atoms are identifiers starting with a lower-case letter (e.g. `alice`, `edge`); they map to the `Term::Atom` variant. Numeric/constant literals are not accepted by the parser (the WatDiv `c<id>` constants are an atom-name convention, not numeric tokens).
- `_` is a placeholder for an unused position.
- A query is terminated with a period.
- Body predicate names must match a `relations[].name` declared above.

## Minimal example (`triangle.yml`)

The smallest complete benchmark, and the one to copy when starting a new one. Its
relation is committed rather than fetched, so it runs offline on a cold cache:

```yaml
name: triangle
description: "Triangle query over a small committed edge relation (sanity workload)"
relations:
  - name: edge
    path: "benchmarks/data/triangle/edge.csv"
queries:
  - name: triangle
    description: "Three-way cyclic join; four matches over the committed graph"
    query: "T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z)."
```

`benchmarks/data/triangle/edge.csv` is a directed graph on five vertices with eight
edges, chosen so the result can be checked by hand — the file's comments list the four
expected matches and two near-misses. Run it with:

```sh
kermit bench run triangle -i tree-trie -a leapfrog-triejoin
```

or, to see the answers rather than the timings:

```sh
kermit join -q <(echo 'T(X, Y, Z) :- edge(X, Y), edge(Y, Z), edge(X, Z).') \
  -r benchmarks/data/triangle/edge.csv -i tree-trie -a leapfrog-triejoin
```

## Multi-relation, multi-query example

See `oxford-uniform-s1.yml` for a benchmark with 8 relations and 3 queries (`binary-join`, `triangle`, `six-way`). Each query shares the same relation set, so downloading the data once amortises across all queries.

## Declarative generator example (WatDiv)

```yaml
name: watdiv-100-stress1
description: "WatDiv stress, scale 100"
generator:
  kind: watdiv
  scale: 100
  stress:
    max_query_size: 5
    query_count: 20
    constants_per_query: 2
    allow_join_vertex: false
```

`bench run watdiv-100-stress1` will materialise the data on first invocation. Subsequent runs short-circuit on the cached `meta.json` if the spec hash matches; if you edit the YAML's params, the next run errors with `SpecDrift` instead of silently regenerating — re-run with `--force` to opt into regeneration.

```yaml
name: lubm-1-q1q3q5
description: "LUBM(1, 0), three queries only"
generator:
  kind: lubm
  scale: 1
  queries: [q1, q3, q5]
```

## Frozen WatDiv snapshots (`watdiv-stress-*.yml`)

The committed `watdiv-stress-*.yml` files are **frozen snapshots**, not declarative specs. They were produced by `scripts/watdiv-preprocess/` (Python) before the on-the-fly path existed. Do not edit them by hand — regenerate via the preprocessor. The on-the-fly path (`generator: { kind: watdiv, ... }`) supersedes this for new workloads.

Every WatDiv query body may contain `c<dict-id>` atom terms filtering a BGP position against a constant URI. At join time, the join entry point (`lftj_join` / `hash_join`) rewrites each atom into a fresh variable + synthetic `Const_c<id>` unary relation (Veldhuizen 2014 §3.4 point 4 — see `kermit-algos/src/const_rewrite.rs`).

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
