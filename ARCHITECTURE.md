# Kermit Architecture

Kermit is a Rust library for relational algebra research and benchmarking. It was created as a platform for a Masters thesis investigating the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481) across different data structures.

## Design Goals

1. **Algorithm-Data Structure Decoupling**: Join algorithms should work with any data structure that implements the required iterator traits
2. **Extensibility**: New data structures and algorithms can be added without modifying existing code
3. **Benchmarking**: First-class support for performance comparison across implementations
4. **Safety**: Entirely safe Rust with no unsafe blocks

## Workspace Structure

```
kermit/
├── kermit-iters/    # Core iterator traits (no dependencies)
├── kermit-derive/   # Proc macros for iterator boilerplate
├── kermit-parser/   # Datalog query parser
├── kermit-ds/       # Data structures (tries, relations)
├── kermit-algos/    # Join algorithms
├── kermit-bench/    # Benchmark definitions, discovery, caching
├── kermit-rdf/      # RDF/SPARQL pipelines (WatDiv, LUBM generators)
└── kermit/          # CLI binary and top-level integration
```

### Dependency Graph

```
kermit-iters ◄─── kermit-derive
     │
     ├──────────── kermit-parser
     │                  │
     ▼                  ▼
kermit-ds ◄─────── kermit-algos       kermit-bench   (isolated)
                        │                  │
                        ▼                  │
                   kermit-rdf ◄────────────┤
                        │                  │
                        └────────┬─────────┘
                                 ▼
                              kermit
```

`kermit-bench` has no internal kermit dependencies. `kermit-rdf` depends on `kermit-parser`, `kermit-ds`, and `kermit-bench`. The `kermit` binary depends on every other crate.

## Core Abstractions

### Iterator Traits (`kermit-iters`)

The foundation of Kermit is a hierarchy of iterator traits that abstract over how data structures are traversed:

```
JoinIterable (marker trait)
     │
     ├── LinearIterable ──► LinearIterator
     │
     └── TrieIterable ────► TrieIterator : LinearIterator
```

#### LinearIterator

A seek-based iterator for sorted sequences:

```rust
pub trait LinearIterator {
    fn key(&self) -> Option<usize>;      // Current position's key
    fn next(&mut self) -> Option<usize>; // Advance and return key
    fn seek(&mut self, seek_key: usize) -> bool; // Jump to key ≥ seek_key
    fn at_end(&self) -> bool;            // Check if exhausted
}
```

The `seek` operation is critical for the leapfrog algorithm's efficiency—it allows jumping past irrelevant keys rather than scanning linearly.

#### TrieIterator

Extends `LinearIterator` with hierarchical navigation:

```rust
pub trait TrieIterator: LinearIterator {
    fn open(&mut self) -> bool;  // Descend to children of current key
    fn up(&mut self) -> bool;    // Ascend to parent level
}
```

This enables depth-first traversal of trie structures, which is essential for multi-way joins where we need to explore matching prefixes across multiple relations.

#### TrieIteratorWrapper

Converts any `TrieIterator` into a standard Rust `Iterator<Item = Vec<usize>>` that yields complete tuples. It handles the stack management for depth-first traversal automatically.

### Data Structures (`kermit-ds`)

#### Relation Trait

The core abstraction for relational data:

```rust
pub trait Relation: JoinIterable + Projectable {
    fn header(&self) -> &RelationHeader;
    fn new(header: RelationHeader) -> Self;
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self;
    fn insert(&mut self, tuple: Vec<usize>) -> bool;
    fn insert_all(&mut self, tuples: Vec<Vec<usize>>) -> bool;
}
```

`RelationHeader` carries metadata: relation name, attribute names, and arity.

#### TreeTrie

A traditional pointer-based trie where each node contains:
- A key value
- A sorted vector of child nodes

```rust
struct TrieNode {
    key: usize,
    children: Vec<TrieNode>,
}

struct TreeTrie {
    header: RelationHeader,
    children: Vec<TrieNode>,
}
```

Tuples are stored as root-to-leaf paths. Children are kept sorted for binary search during seeks.

#### ColumnTrie

A column-oriented trie inspired by the [Nemo rule engine](https://github.com/knowsys/nemo). Instead of pointer-based nodes, it uses parallel arrays:

```rust
struct ColumnTrieLayer {
    data: Vec<usize>,      // Keys at this level, sorted within intervals
    interval: Vec<usize>,  // Start indices for each parent's children
}

struct ColumnTrie {
    header: RelationHeader,
    layers: Vec<ColumnTrieLayer>,
}
```

For a 3-ary relation, there are 3 layers. The `interval` array maps each key in layer N to the range of its children in layer N+1. This representation is more cache-friendly for large datasets.

### Query Representation (`kermit-parser`)

Queries follow Datalog syntax and are parsed into an AST:

```rust
enum Term {
    Var(String),      // Uppercase: X, Y, Person
    Atom(String),     // Lowercase: alice, bob
    Placeholder,      // Underscore: _
}

struct Predicate {
    name: String,     // Relation name
    terms: Vec<Term>,
}

struct JoinQuery {
    head: Predicate,  // Result schema
    body: Vec<Predicate>, // Relations to join
}
```

Example: `ancestor(X, Z) :- parent(X, Y), parent(Y, Z).`

- Head: `ancestor(X, Z)` — defines output columns
- Body: `parent(X, Y), parent(Y, Z)` — relations to join
- Variable `Y` appears in both body predicates, creating a join condition

## Algorithms (`kermit-algos`)

### Leapfrog Join

The `LeapfrogJoinIter` implements intersection of multiple sorted iterators. Given k iterators positioned at keys, it finds common keys by:

1. Sort iterators by their current key
2. Seek the first iterator to the last iterator's key
3. If they match, we found a common key
4. Otherwise, repeat from step 2

This "leapfrog" pattern avoids examining every element—iterators jump past non-matching regions.

### Leapfrog Triejoin

`LeapfrogTriejoinIter` extends leapfrog join to work with trie iterators for multi-way joins. It coordinates multiple trie iterators, one per relation:

1. **Variable Ordering**: Variables are numbered by first appearance in head, then body
2. **Iterator Assignment**: Each variable level knows which relation iterators participate
3. **Level-by-Level Join**: At each trie depth, a leapfrog join finds matching keys
4. **Navigation**: `triejoin_open()` descends all participating iterators; `triejoin_up()` ascends

The algorithm efficiently handles queries like:
```
Q(A, B, C) :- R(A, B), S(B, C), T(A, C).
```

At depth 0 (variable A): R and T participate
At depth 1 (variable B): R and S participate
At depth 2 (variable C): S and T participate

### JoinAlgo Trait

```rust
pub trait JoinAlgo<DS> where DS: JoinIterable {
    fn join_iter(
        query: JoinQuery,
        datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>>;
}
```

This abstraction allows implementing different join algorithms that work with any join-iterable data structure.

### Const-Rewrite

Before handing a query to `JoinAlgo::join_iter`, `DatabaseEngine::join` calls `kermit_algos::rewrite_atoms` (see `kermit-algos/src/const_rewrite.rs`) to implement Veldhuizen 2014 §3.4 point 4. Each `Term::Atom("c<id>")` in the body becomes a fresh variable `K<i>` plus a synthetic unary predicate `Const_c<id>(K<i>)` appended to the body, backed by a `SingletonTrieIter`. Body atoms only — head atoms are passed through. Implication: a new `JoinAlgo` impl must tolerate seeing the rewritten query, which can carry extra unary body predicates that do not appear in the user's original Datalog source. Adding a new data structure does *not* require any atom handling — the rewrite happens above the DS layer.

## Benchmarking (`kermit-bench`)

The benchmark crate is a leaf with no internal kermit dependencies. It defines the YAML schema, performs discovery, and manages download / cache state for relation files. Criterion execution itself lives in the `kermit` binary.

### YAML benchmark definitions

A benchmark is a single `benchmarks/<name>.yml` file (filename stem must equal the `name:` field). The schema is documented in `benchmarks/README.md`. Two flavours:

- **Static** — declares `relations:` (each with a download URL) and `queries:` (Datalog strings). See `benchmarks/triangle.yml`.
- **Generator-driven** — declares a `generator: { kind: watdiv|lubm, scale: N, ... }` block instead. The relations and queries are produced on demand by a `kermit-rdf` pipeline.

The two are mutually exclusive; `BenchmarkDefinition::validate` enforces the XOR plus structural invariants (non-empty name, unique relation/query names, portable filename characters, generator-specific bounds).

### Discovery

`discovery::load_all_benchmarks(workspace_root)` reads every `*.yml`/`*.yaml` under `benchmarks/`. `discovery::load_all_benchmarks_with_cache(workspace_root, cache_root)` additionally walks `<cache_root>/<name>/`, treating any subdirectory containing both `benchmark.yml` AND `meta.json` as a generator-produced benchmark. Cache entries override workspace entries on name collision.

### Cache layout and spec-hash drift

Generator-driven benchmarks materialise into:

```
~/.cache/kermit/benchmarks/<name>/
  meta.json        # PipelineMeta or LubmMeta with `spec_hash` field
  benchmark.yml    # BenchmarkDefinition with file:// relation URLs
  dict.parquet
  <predicate>.parquet × N
  raw/...
  expected/...
```

`GeneratorSpec::spec_hash` is a SHA-256 over the canonical YAML serialisation of the spec. On `bench run <name>`, `kermit/src/materialize.rs::materialize` compares the cached `meta.json.spec_hash` against the current YAML's hash:

- **Match** → load cache-side `benchmark.yml` and short-circuit.
- **Mismatch** without `--force` → return `BenchError::SpecDrift` (regenerating a multi-minute pipeline silently is not desired).
- **Mismatch** with `--force` → wipe the cache subdir and re-run the pipeline.

Legacy `meta.json` files lacking `spec_hash` are treated as drift.

## RDF/SPARQL Pipelines (`kermit-rdf`)

`kermit-rdf` provides on-the-fly benchmark generation from RDF generators. Two pipelines:

- **WatDiv** — `pipeline::run_pipeline` drives the vendored `kermit-rdf/vendor/watdiv` binary (CLI: `-d <model> <scale>` for data, `-s ... ` for stress-template queries). The binary writes only to stdout; `driver::invoke` captures it and splits on `#end` markers. The binary is gitignored — build locally; surrounding `MODEL.txt`/`files/`/`VERSION` are committed.
- **LUBM** — `lubm::pipeline::run_lubm_pipeline` drives `vendor/lubm-uba/lubm-uba.jar` (committed, ~2.9 MB), gunzips the resulting `Universities.nt.gz`, then runs Univ-Bench TBox forward chaining via `lubm::entailment` before partitioning. Requires JDK 8 on PATH. The 14 LUBM queries are committed verbatim at `kermit-rdf/queries/lubm/q1.sparql … q14.sparql` and exposed via `lubm::queries::lubm_query_specs`.

Both pipelines share post-driver stages: `partition` (split N-Triples by predicate), `parquet` (encode dict + per-predicate parquet), `dict` (string→usize), `sparql::translator` (BGP-only SPARQL → Datalog), `yaml_emit` (write the cache-side `benchmark.yml`), `expected` (cardinality CSVs). The cache-side YAML always has `generator: None` — provenance lives in `meta.json.spec_hash`.

User-facing reference docs live at `docs/benchmarks/WATDIV.md` and `docs/benchmarks/LUBM.md`. Module-internal contributor notes for the LUBM driver live at `kermit-rdf/src/lubm/README.md`.

## CLI (`kermit`)

The binary exposes two top-level subcommands:

- `kermit join` — execute a Datalog query against relation files and print the result tuples.
- `kermit bench` — Criterion-driven benchmarking, with the following subcommands:
  - `bench join` — wrap a join in Criterion timing.
  - `bench ds` — measure insertion / iteration / heap size for a single relation file against one or more index structures.
  - `bench run [<NAME> | --all]` — run YAML-defined benchmarks; `--metrics` selects from `insertion`, `iteration`, `space`. `--force` opts into regenerating a generator-driven benchmark when its spec hash drifts.
  - `bench list` — list benchmarks; status distinguishes `cached` / `not cached` for static and `not generated` / `cached` / `stale` for generator-driven.
  - `bench fetch [<NAME>]` — download relation parquets for static benchmarks.
  - `bench clean [<NAME>]` — remove cached benchmark artefacts.
  - `bench gen { watdiv | lubm }` — imperative on-the-fly generation, bypassing YAML; writes into the same cache layout under a user-supplied `--tag`.

`--indexstructure` and `--algorithm` accept `all` on `bench ds` and `bench run` for Cartesian sweeps. Working examples live in `README.md` and `USAGE.md`; the YAML schema and generator-spec details live in `benchmarks/README.md`.

### Space measurement

`kermit/src/measurement.rs` defines a custom Criterion `Measurement` (`SpaceMeasurement`) plus a `BytesFormatter` that picks a binary-prefixed unit (`B`/`KiB`/`MiB`/`GiB`). When `--metrics space` is requested, both `bench ds` and `bench run` route through `Criterion<SpaceMeasurement>` via `iter_custom`, producing `target/criterion/{group}/{dir}/...` JSON alongside the time metrics. The per-DS hook is the `HeapSize` trait (`heap_size_bytes()`); the closure calls it once per iter on a pre-built relation.

### JSON bench reports

Every `kermit bench` invocation writes a `BenchReport` JSON array to disk. The default path is `bench-runs/{kind}-{unix-millis}.json` (the directory is auto-created and gitignored at the workspace root); pass `--report-json <PATH>` to override. Each report carries `metadata` (label/value pairs mirroring stderr), `axes` (a structured map for tooling: `data_structure`, `algorithm`, `query`, `tuples`, …), and `criterion_groups` pointers resolving to per-function `target/criterion/{group}/{dir}/` artefacts. The schema is versioned by `schema_version` (currently `2`) and lives in `kermit/src/bench_report.rs`; the full key catalogue is documented in `docs/specs/bench-report-schema.md`.

## File I/O

Relations can be loaded from:
- **CSV**: Header row defines attribute names, filename becomes relation name
- **Parquet**: Schema provides attribute names, efficient columnar storage

The `RelationFileExt` trait provides `from_csv()` and `from_parquet()` methods via blanket implementation for any `Relation`.

## Key Type

All keys are `usize`. String values must be dictionary-encoded before use. This simplifies the implementation and improves performance for join comparisons.

## Adding New Components

### New Data Structure

1. Implement `Relation` + `TrieIterable` + `HeapSize` in `kermit-ds`.
2. Provide a corresponding `TrieIterator` type.
3. Add a variant to the `IndexStructure` enum (in `kermit-ds`) for CLI selection.
4. Add the corresponding match arms in `instantiate_database` (`kermit/src/db.rs`) and the `run_ds_bench` / `run_benchmark` dispatch in `kermit/src/main.rs`.

### New Join Algorithm

1. Implement `JoinAlgo<DS>` in `kermit-algos`. The implementation must tolerate the const-rewritten query shape (extra synthetic unary body predicates).
2. Add a variant to the `JoinAlgorithm` enum for CLI selection.
3. Wire the new variant into `instantiate_database`.

### New Benchmark

1. Create `benchmarks/<name>.yml`. The schema is documented in `benchmarks/README.md`.
   - For a static benchmark, declare `relations:` (with download URLs) and `queries:` (Datalog strings).
   - For a generator-driven benchmark, declare `generator: { kind: watdiv | lubm, scale: N, ... }` instead.
2. Run `kermit bench list` to verify discovery and validate the YAML.
3. For static benchmarks, `kermit bench fetch <name>` downloads the relation parquets. For generator-driven benchmarks, `kermit bench run <name>` materialises them on first invocation.
