# Kermit Architecture

Kermit is a Rust library for relational algebra research and benchmarking. It was created as a platform for a Masters thesis investigating the [Leapfrog Triejoin algorithm](https://arxiv.org/abs/1210.0481) across different data structures.

## Design Goals

1. **Algorithm-Data Structure Decoupling**: Join algorithms should work with any data structure that implements the required iterator traits
2. **Extensibility**: New data structures and algorithms should be added by following a fixed recipe rather than by reworking existing components
3. **Benchmarking**: First-class support for performance comparison across implementations
4. **Safety**: Entirely safe Rust with no unsafe blocks

Goal 2 is about *recognisable, additive* extension, not zero edits. Adding a component does require touching a known set of registration points — the `IndexStructure` / `JoinAlgorithm` / `Optimiser` enums, the CLI selectors and dispatch, and the test suites. Those sites are enumerated under "Adding New Components"; the contract is that the list does not grow and that siblings are left alone.

## Workspace Structure

```
kermit/
├── kermit-iters/    # Core iterator traits (no dependencies)
├── kermit-derive/   # Proc macros for iterator boilerplate
├── kermit-parser/   # Datalog query parser
├── kermit-ds/       # Data structures (tries, relations)
├── kermit-algos/    # Join algorithms + query optimisers
├── kermit-bench/    # Benchmark definitions, discovery, caching
├── kermit-rdf/      # RDF/SPARQL pipelines (WatDiv, LUBM generators)
├── kermit/          # CLI binary and top-level integration
└── python/
    └── kermit-lab/  # Analysis library over the JSON bench reports (not a Cargo member)
```

### Dependency Graph

Arrows read "depends on". Only production (`[dependencies]`) edges are listed.

```
kermit         ──▶ kermit-iters, kermit-ds, kermit-algos,
                   kermit-parser, kermit-bench, kermit-rdf
kermit-rdf     ──▶ kermit-bench
kermit-algos   ──▶ kermit-iters, kermit-derive, kermit-parser
kermit-ds      ──▶ kermit-iters, kermit-derive
kermit-derive  ──▶ (none)
kermit-parser  ──▶ (none)
kermit-bench   ──▶ (none)
kermit-iters   ──▶ (none)
```

Three crates are leaves with no internal dependencies: `kermit-iters`, `kermit-parser`, and `kermit-bench`.

`kermit-algos` is decoupled from the data structures (per Design Goal #1). The `JoinAlgo<DS>` trait is generic over `DS: JoinIterable`; each implementation narrows that bound to the iterator family it needs (`TrieIterable` or `HashTrieIterable`, see "Two Iterator Families" below). `kermit-ds` appears only as a `[dev-dependencies]` entry for the crate's own tests, so there is no production `kermit-algos → kermit-ds` edge.

`kermit-derive` is a proc-macro crate; its `kermit-iters` dependency is dev-only, used by its integration tests. The `kermit` binary depends on every sibling except `kermit-derive`, which it pulls in transitively via `kermit-ds`.

`kermit-rdf` depends on `kermit-bench` alone. Its coupling to the query layer is deliberately textual, not structural: `sparql::translator` emits a Datalog *string* into the generated `benchmark.yml`, and that string is parsed by `kermit-parser` later, in the binary, when the benchmark is actually run. The pipeline also never constructs a relation, so it has no need of `kermit-ds`. (Both crates were declared in its manifest for a period without ever being referenced; the entries were removed.)

## Core Abstractions

### Iterator Traits (`kermit-iters`)

The foundation of Kermit is a hierarchy of iterator traits that abstract over how data structures are traversed:

```
JoinIterable (marker trait, no methods)
     │
     ├── LinearIterable ───► LinearIterator
     │
     ├── TrieIterable ─────► TrieIterator : LinearIterator
     │                         (sorted keys, least-upper-bound `seek`)
     │
     └── HashTrieIterable ─► HashTrieIterator
                               (hashed keys, exact-match `lookup`)
```

#### Two Iterator Families

`TrieIterator` and `HashTrieIterator` are **parallel families, not a hierarchy**. They share only the empty `JoinIterable` marker.

The split is deliberate. `LinearIterator::seek` has least-upper-bound semantics, which requires sorted data; hash navigation is exact-match. The two contracts cannot be satisfied by one trait, so `HashTrieIterator` is a fork rather than an extension (see the rationale in `kermit-iters/src/hash_trie.rs`, citing the SIGMOD 2020 hash-trie-join paper).

The consequence is that the fork propagates up every layer of the stack, and each layer must be split by hand:

| Layer | Sorted family | Hash family |
|---|---|---|
| Iterator | `TrieIterator` | `HashTrieIterator` |
| Iterable | `TrieIterable` | `HashTrieIterable` |
| Structure | `TreeTrie`, `ColumnTrie` | `HashTrie<H>` |
| Algorithm | `LeapfrogTriejoin` | `HashTriejoin` |
| `Projectable` | `project_via_trie_iter` (shared helper) | hand-rolled on `HashTrie` |
| Engine | `lftj_join` free function over `BTreeMap<String, R>` | `hash_join` free function over `BTreeMap<String, HashTrie<H, P>>` |
| Bench cell | `Execution::TrieLftj(SortedTrie)` / `TrieLftj<R>` | `Execution::HashHtj { hasher, pruning, config }` / `HashHtj<H, P>` (labels derived from `H`/`P`) |
| `bench run` dispatch | one generic `run_benchmark<F: ExecutionFamily>` | the same `run_benchmark<F>` |
| `bench ds` dispatch | one generic `run_ds_bench<F: RelationFamily>` over `SortedTrieFamily<R>` | the same `run_ds_bench<F>` over `HashTrieFamily<H, P>` |

Only three `(index structure, algorithm)` pairs are valid: `(TreeTrie, LeapfrogTriejoin)`, `(ColumnTrie, LeapfrogTriejoin)`, and `(HashTrie, HashTriejoin)`. The type system enforces this at every layer, the CLI included: user strings are resolved into two independent enums, but `Execution::for_pair` is the only bridge from that pair back to a runnable cell — see "Selector dispatch" under CLI.

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

#### HashTrieIterator

The hash-family counterpart. Keys are `u64` hashes rather than `usize` values, positioning is exact-match, and `size` exposes the bucket count so the join can scan the smallest table:

```rust
pub trait HashTrieIterator {
    fn key(&self) -> Option<u64>;
    fn next(&mut self) -> Option<u64>;
    fn lookup(&mut self, hash: u64) -> bool; // exact-match, not upper-bound
    fn size(&self) -> usize;                 // bucket count at current node
    fn at_end(&self) -> bool;
    fn open(&mut self) -> bool;
    fn up(&mut self) -> bool;
    fn leaf_tuples(&self) -> Option<&[Vec<usize>]>;
}
```

Because a per-level key is a hash rather than a value, `HashTrieIterable` does *not* require `IntoIterator<Item = Vec<usize>>` the way `TrieIterable` does — hash traversal is not naturally tuple-shaped. `HashTrie` exposes `collect_tuples()` for materialisation instead, and hashing means a descent can produce false positives, so the join must verify shared variables against the real tuples at the leaf.

#### TrieIteratorWrapper

Converts any `TrieIterator` into a standard Rust `Iterator<Item = Vec<usize>>` that yields complete tuples. It handles the stack management for depth-first traversal automatically. `#[derive(IntoTrieIter)]` (from `kermit-derive`) generates the `IntoIterator` impl that wraps an iterator type in it. There is no hash-family equivalent, per the note above.

### Data Structures (`kermit-ds`)

#### Relation Trait

The core abstraction for relational data:

```rust
pub trait Relation: JoinIterable + Projectable {
    fn header(&self) -> &RelationHeader;
    fn new(header: RelationHeader) -> Self;
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<usize>>) -> Self;
    fn insert(&mut self, tuple: Vec<usize>);
    fn insert_all(&mut self, tuples: Vec<Vec<usize>>);
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
    tuple_count: usize, // distinct tuples; backs `Cardinality`
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
    tuple_count: usize,
}
```

For a 3-ary relation, there are 3 layers. The `interval` array maps each key in layer N to the range of its children in layer N+1. This representation is more cache-friendly for large datasets. The trade-off is insertion cost: inserting a key in an early layer shifts every subsequent interval offset.

#### HashTrie

A hash-based trie, generic over a `HashStrategy` layout parameter. Each level is an open-addressing hash table keyed on the hash of that attribute's value:

```rust
struct HashTrie<H: HashStrategy = SipHashStrategy> {
    header: RelationHeader,
    root: HashTrieNode,
    tuple_count: usize,     // multiset: duplicates count
    _hasher: PhantomData<H>,
}

enum HashTrieNode {
    Inner(HashTable<HashTrieNode>),    // child nodes
    Leaf(HashTable<Vec<Vec<usize>>>),  // full materialised tuple chains
}
```

Tables use linear probing with power-of-two capacity, doubling above a 0.7 load factor. Two differences from the sorted tries matter:

- **Multiset semantics.** Tuples with identical hash signatures chain in the same leaf bucket rather than deduplicating, so `Cardinality::tuple_count` counts multiset size where `TreeTrie` and `ColumnTrie` count distinct tuples.
- **Leaves hold whole tuples.** Because inner levels store only hashes, the real values are needed at the leaf to reject hash collisions.

`H` is the crate's only optimization axis: `SipHashStrategy` (default) and `FxHashStrategy` are zero-sized types implementing both `HashStrategy` and `LayoutOption`, and `HashTrie<H>` is the sole `HasOptimizationAxes` implementor in the workspace, reporting `ds_layout_hasher`.

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

1. **Variable Numbering**: Canonical variable indices are assigned by first appearance in head, then body (`kermit_algos::analyse`). The actual descent order — which may differ from this numbering — is chosen by the query's `QueryOptimiser` and delivered as a `QueryPlan` (see "Query Planning" below).
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

Note one documented deviation from the paper: Veldhuizen keeps one persistent leapfrog per level over freely-aliased iterator arrays. Safe Rust cannot alias owned iterators across levels, so `LeapfrogTriejoinIter` instead *moves* iterators between an idle pool and the inner `LeapfrogJoinIter` on every depth change. Observable semantics are preserved; the cost is a drain/refill per `open`/`up`. The rationale is recorded in the module docs of `kermit-algos/src/leapfrog_triejoin.rs`.

### Hash Triejoin

`HashTriejoin` implements Algorithm 3 of the SIGMOD 2020 hash-trie-join paper, over the `HashTrieIterable` family. Unlike `LeapfrogTriejoin` it is not an iterator type — it is a recursive `enumerate` over attribute positions:

1. **Descend**: open every iterator participating at this attribute position.
2. **Choose a driver**: pick the iterator with the smallest hash table (`argmin size`) and scan it.
3. **Probe**: `lookup` that hash in every other participating iterator; recurse on a match.
4. **Emit**: at the leaf, cross-product the participating tuple chains, then **verify shared variables against the real tuple values** — hashing can produce false positives at any inner level, so equality must be re-checked before a tuple is emitted.
5. **Ascend**: `up` exactly as many times as the descent opened, including on partial-open failure.

One behavioural asymmetry is worth knowing: `HashTriejoin::join_iter` fully materialises its results into a `Vec` before returning `impl Iterator`, whereas `LeapfrogTriejoin` returns a genuinely lazy iterator chain. This does not currently affect benchmarks, because `ExecutionFamily::join` returns `Vec<Vec<usize>>` and normalises both — but it means LFTJ's laziness is never exploited by the CLI, and any future time-to-first-tuple metric would be comparing unlike things.

### JoinAlgo Trait

```rust
pub trait JoinAlgo<DS> where DS: JoinIterable {
    fn join_iter(
        plan: &QueryPlan,
        query: JoinQuery,
        datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>>;
}
```

This abstraction allows implementing different join algorithms that work with any join-iterable data structure. `plan` supplies the variable descent order (see "Query Planning" below); implementations validate it against the query with `QueryPlan::validate` and panic if it is inconsistent.

The trait bound is the weak `JoinIterable` marker; each implementation narrows it to the family it can actually traverse:

```rust
impl<DS: TrieIterable>     JoinAlgo<DS> for LeapfrogTriejoin { … }
impl<DS: HashTrieIterable> JoinAlgo<DS> for HashTriejoin     { … }
```

That is what makes `(HashTrie, LeapfrogTriejoin)` a compile error rather than a runtime concern — everywhere except the CLI, which erases both sides into enums.

### Const-Rewrite

Before handing a query to `JoinAlgo::join_iter`, the shared join body behind `lftj_join` / `hash_join` calls `kermit_algos::rewrite_atoms` (see `kermit-algos/src/const_rewrite.rs`) to implement Veldhuizen 2014 §3.4 point 4. Each `Term::Atom("c<id>")` in the body becomes a fresh variable `K<i>` plus a synthetic unary predicate `Const_c<id>(K<i>)` appended to the body, backed by a `SingletonTrieIter`. Body atoms only — head atoms are passed through. Implication: a new `JoinAlgo` impl must tolerate seeing the rewritten query, which can carry extra unary body predicates that do not appear in the user's original Datalog source. Adding a new data structure does *not* require any atom handling — the rewrite happens above the DS layer.

### Query Planning

Between the const-view rewrite and execution, the engine plans the join: it gathers per-relation tuple counts (`Cardinality::tuple_count`) into `CatalogStats` and asks its `QueryOptimiser` for a `QueryPlan` — the global attribute order the algorithm will descend. The space of valid plans is exactly the set of topological orders of the column-order constraint DAG; provided optimisers rank candidates inside Kahn's algorithm and are valid by construction, and `join_iter` asserts `QueryPlan::validate` defensively. `LexicographicOptimiser` (default) reproduces the historical hardcoded order; `CardinalityOptimiser` prefers variables from small relations (`--optimiser cardinality`).

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

- **WatDiv** — `pipeline::run_pipeline` drives the vendored `kermit-rdf/vendor/watdiv` binary (CLI: `-d <model> <scale>` for data, `-s ... ` for stress-template queries). The binary writes only to stdout; `driver::invoke` captures it and splits on `#end` markers. The binary is committed (vendored, ~360 KB at `bin/Release/watdiv`, force-added past the inert `**/watdiv` ignore rule); `MODEL.txt`/`files/`/`VERSION` are committed alongside it.
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

Working examples live in `README.md` and `USAGE.md`; the YAML schema and generator-spec details live in `benchmarks/README.md`.

### Database layer

`kermit/src/db.rs` exposes one join entry point per iterator family, both free functions over a `BTreeMap<String, R>` relation store keyed by relation name: `lftj_join<R: TrieIterable + Cardinality, JA>` (generic in the sorted-family algorithm) and `hash_join<R: HashTrieIterable + Cardinality, H: HashStrategy>` (hardwired to `HashTriejoin`; `H` hashes constant singletons with the same strategy the relations were built with). Each runs the const-rewrite, plans the query, and returns `Vec<Vec<usize>>`.

They share a single private body. The `JoinFamily<R>` trait — with a GAT `Wrapper<'a>` — abstracts the only two steps that differ between families: wrapping a borrowed relation (`TrieIterKind` vs `HashTrieIterKind`) and wrapping a constant atom (the hash side folds in `H::hash`). Everything else — `rewrite_atoms`, the wrapper map, `CatalogStats`, `optimiser.plan`, `join_iter(..).collect()` — is written once, so a fix to the prologue cannot land in one family only.

There is deliberately no object-safe engine trait. Runtime selection of the `(structure, algorithm)` cell — for `kermit join` and `bench join` as much as for `bench run` — goes through `Execution::for_pair` and the `ExecutionFamily` impls in `kermit/src/execution.rs` (`load_query_runner` in `main.rs` monomorphises per cell exactly as `dispatch_run_bench` does). An incompatible pair is a usage error, and all three valid cells, including `(HashTrie, HashTriejoin)`, are reachable from every command.

### Selector dispatch

`bench ds` and `bench run` accept `all` for `--indexstructure` and `--algorithm`, expanding to a Cartesian sweep.

For `bench run`, that sweep is expressed as *cells* rather than pairs. `kermit/src/execution.rs` defines `Execution`, an enum whose variants each fix **both** halves of the combination — `TrieLftj(TreeTrie | ColumnTrie)` and `HashHtj { hasher, pruning, config }` — so an `Execution` cannot describe something the CLI is unable to run. `Execution::for_pair` is the sole constructor and returns `None` for the three incompatible pairs; `Sweep::expand` partitions the cross product into `cells` and a `skipped` list.

Consequently `-i all -a all` runs exactly the three valid cells, announcing each skipped pair on stderr, while a single explicitly-named incompatible pair leaves nothing to run and is reported as a usage error. Because the report's `data_structure` and `algorithm` axes are both derived from `ExecutionFamily::execution()`, a report cannot name an algorithm it did not run (issue #56).

`bench ds` selects a structure but no algorithm, so it names its cell through the total `Execution::for_structure` (every structure has exactly one compatible algorithm) and runs the same kind of generic runner, `run_ds_bench<F: RelationFamily>`. `RelationFamily` is the relation-facing supertrait of `ExecutionFamily` (load, tuples, axes, cell label); `bench ds` instantiates the structure-only markers `SortedTrieFamily<R>` / `HashTrieFamily<H, P>`, which implement the supertrait alone and so cannot join. The join families `TrieLftj<R>` / `HashHtj<H, P>` embed those markers and delegate, so the two commands cannot disagree about a structure (issue #61). `HashTrieFamily<H, P>` is also where the `--ds-config` values live, so the `insertion` and `end_to_end` closures build through `RelationFamily::build_relation` and measure the structure the report's `ds_config_*` axes describe.

### Space measurement

`kermit/src/measurement.rs` defines a custom Criterion `Measurement` (`SpaceMeasurement`) plus a `BytesFormatter` that picks a binary-prefixed unit (`B`/`KiB`/`MiB`/`GiB`). When `--metrics space` is requested, both `bench ds` and `bench run` route through `Criterion<SpaceMeasurement>` via `iter_custom`, producing `target/criterion/{group}/{dir}/...` JSON alongside the time metrics. The per-DS hook is the `HeapSize` trait (`heap_size_bytes()`); the closure calls it once per iter on a pre-built relation.

### JSON bench reports

Every `kermit bench` invocation writes a `BenchReport` JSON array to disk. The default path is `bench-runs/{kind}-{unix-millis}.json` (the directory is auto-created and gitignored at the workspace root); pass `--report-json <PATH>` to override. Each report carries `metadata` (label/value pairs mirroring stderr), `axes` (a structured map for tooling: `data_structure`, `algorithm`, `query`, `tuples`, …), and `criterion_groups` pointers resolving to per-function `target/criterion/{group}/{dir}/` artefacts. The schema is versioned by `schema_version` (currently `2`) and lives in `kermit/src/bench_report.rs`; the full key catalogue is documented in `docs/specs/bench-report-schema.md`.

## Analysis (`python/kermit-lab`)

`python/kermit-lab/` is a uv-managed Python package for notebook-first analysis of benchmark output. It is not a Cargo workspace member and shares no code with the Rust side — **the only coupling is the on-disk data contract**:

- `kl.load("bench-runs/*.json")` parses `BenchReport` arrays into a pandas `DataFrame`, including the `ds_*` / `algo_*` optimization axes. It checks `schema_version` on load and raises `SchemaError` if a report is newer than the package understands.
- Each `criterion_groups` pointer is then resolved into `target/criterion/{group}/{directory_name}/new/*.json`, matching on `benchmark.json`'s `function_id` rather than recomputing Criterion's name escaping.
- `kl.plot(df, kind=…, x=…, colour=…, facet=…)` is the general engine; `kl.scaling()`, `kl.bar_time()`, `kl.ablation()` and friends are presets over it, each returning a `matplotlib.figure.Figure`. `kl.summary` / `compare` / `bootstrap_ratio_ci` / `mannwhitney_u` cover pivots and statistics.

Because the boundary is a versioned file format rather than a binding, analysis code and notebooks stay valid across Rust revisions. Bump `schema_version` in `bench_report.rs` on any breaking field-name or value-type change, and update `SCHEMA_VERSION` in `kermit_lab` to match.

## File I/O

Relations can be loaded from:
- **CSV**: Header row defines attribute names, filename becomes relation name
- **Parquet**: Schema provides attribute names, efficient columnar storage

The `RelationFileExt` trait provides `from_csv()` and `from_parquet()` methods via blanket implementation for any `Relation`.

## Key Type

All keys are `usize`. String values must be dictionary-encoded before use. This simplifies the implementation and improves performance for join comparisons.

## Adding New Components

These are summaries. `CLAUDE.md` holds the authoritative step-by-step recipes, including exact file paths and the rules on scope discipline.

### New Data Structure

1. Create `kermit-ds/src/ds/<name>/` with `mod.rs`, `implementation.rs`, `<name>_iter.rs`.
2. Implement `Relation` + `Projectable` + `HeapSize` + `Cardinality` on the structure. `HeapSize::heap_size_bytes` returns heap bytes only, excluding `size_of::<Self>`.
3. Implement the iterator. For the sorted family that is `TrieIterator` plus `#[derive(IntoTrieIter)]`, then `TrieIterable` on the structure. For the hash family it is `HashTrieIterator` and `HashTrieIterable` — and note that the shared helpers `project_via_trie_iter` and `TrieIteratorWrapper` are `TrieIterable`-only, so a hash-family structure must supply its own equivalents. The `kermit-ds` test macros fork along the same seam: `hash_trie_test_suite!` covers the hash family, `relation_trie_test_suite!` the sorted one.
4. Register the module and add a variant to `IndexStructure` in `kermit-ds/src/ds/mod.rs`.
5. Wire the CLI: a variant on `IndexStructureSelector` and its `expand()`. A sorted-family structure also needs a `SortedTrie` variant plus a `SortedTrieRelation` impl and an `Execution::for_pair` arm (`kermit/src/execution.rs`), and arms in `load_query_runner` and `dispatch_run_bench` / `dispatch_ds_bench` (`kermit/src/main.rs`). A structure in a new trait family needs its own `ExecutionFamily` impl.
6. **Wire the tests.** Add `define_multiway_join_test_suite!(<Type>, <Algo>, LexicographicOptimiser)` and a second invocation with `CardinalityOptimiser` in `kermit/tests/join_tests.rs`, so all 12 standard join patterns run against the structure. Each distinct layout combination is its own suite invocation (e.g. `HashTrieSip`, `HashTrieFx`).
7. **Write the doc** at `docs/data-structures/<name>.md` from the template.

### New Join Algorithm

1. Create `kermit-algos/src/<name>.rs` and implement `JoinAlgo<DS>`, narrowing `DS` to `TrieIterable` or `HashTrieIterable`. The implementation must tolerate the const-rewritten query shape (extra synthetic unary body predicates) and must validate the incoming `QueryPlan`.
2. Register the module and add a variant to the `JoinAlgorithm` enum in `kermit-algos/src/lib.rs`.
3. Wire the CLI: a variant on `JoinAlgorithmSelector` and its `expand()`, plus the algorithm's valid cells in `Execution` / `Execution::for_pair` (the compiler flags the incomplete match) and a `dispatch_run_bench` arm. `kermit join` / `bench join` dispatch through the same cells (`load_query_runner`), so nothing extra is needed for them. A sorted-family algorithm plugs into `lftj_join<R, JA>`; a hash-family one needs its own entry point beside `hash_join` over the shared body (see "Database layer").
4. **Wire the tests.** Add a `define_multiway_join_test_suite!` invocation per compatible index structure × optimiser.
5. **Write the doc** at `docs/algorithms/<name>.md` from the template.

### New Query Optimiser

1. Create `kermit-algos/src/optimiser/<name>.rs` and implement `QueryOptimiser`.
2. Build the ordering with the shared `ordering::topological_order(num_vars, predicate_variables, rank)`. Because ranking only ever chooses among Kahn-ready variables, any rank function yields a valid plan — a policy can be slow, never wrong. Consume statistics via `CatalogStats`, treating a missing entry as "assume large".
3. Register the module and add a variant to the `Optimiser` enum, with `instantiate()` and `axis_value()` arms. The `axis_values_match_clap_value_names` guard test pins axis naming.
4. **Wire the tests.** A `define_multiway_join_test_suite!` invocation per valid (structure, algorithm) pair, plus unit tests for the ranking itself.
5. **Write the doc** at `docs/optimisers/<name>.md` from the template.

### New Benchmark

1. Create `benchmarks/<name>.yml`. The schema is documented in `benchmarks/README.md`.
   - For a static benchmark, declare `relations:` (with download URLs) and `queries:` (Datalog strings).
   - For a generator-driven benchmark, declare `generator: { kind: watdiv | lubm, scale: N, ... }` instead.
2. Run `kermit bench list` to verify discovery and validate the YAML.
3. For static benchmarks, `kermit bench fetch <name>` downloads the relation parquets. For generator-driven benchmarks, `kermit bench run <name>` materialises them on first invocation.
