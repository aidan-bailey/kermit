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
├── kermit-iters/    # Core iterator traits, the tuple batch and the view builds read (no dependencies)
├── kermit-derive/   # Proc macros for iterator boilerplate
├── kermit-parser/   # Datalog query parser
├── kermit-ds/       # Data structures (tries, relations)
├── kermit-algos/    # Join algorithms + query optimisers
├── kermit-bench/    # Benchmark definitions, discovery, caching
├── kermit-rdf/      # RDF/SPARQL pipelines (WatDiv, LUBM generators)
├── kermit-generate/ # Layer shared by the non-RDF generators (H2O #95, SNAP #96, JOB #97)
├── kermit/          # CLI binary and top-level integration
└── python/
    └── kermit-lab/  # Analysis library over the JSON bench reports (not a Cargo member)
```

### Dependency Graph

Arrows read "depends on". Only production (`[dependencies]`) edges are listed.

```
kermit         ──▶ kermit-iters, kermit-ds, kermit-algos,
                   kermit-parser, kermit-bench, kermit-rdf,
                   kermit-generate
kermit-generate ─▶ kermit-ds, kermit-bench, kermit-rdf, kermit-parser
kermit-rdf     ──▶ kermit-bench
kermit-algos   ──▶ kermit-iters, kermit-parser
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

`kermit-generate` is the layer that generators which write their relations themselves, rather than partitioning an RDF tool's N-Triples, join: H2O (#95), SNAP (#96) and JOB (#97). Writing relations needs `kermit-ds`'s `ParquetWriter`, which is exactly the dependency `kermit-rdf` must not take, so they get their own crate. It reuses `kermit-rdf`'s cache contract (`Target`, `GeneratorMeta`, `write_meta_json`, `META_SCHEMA_VERSION`) and `kermit-parser` to check each query it emits. Nothing but the binary depends on it.

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
| Structure | `TreeTrie<S>`, `ColumnTrie<S>` | `HashTrie<H>` |
| Algorithm | `LeapfrogTriejoin` | `HashTriejoin` |
| `Projectable` | `project_via_trie_iter` (shared helper) | hand-rolled on `HashTrie` |
| Engine | `lftj_join` free function over a `Catalog`, building `R` indexes per query | `hash_join` free function over a `Catalog`, building `HashTrie<H, P, E>` indexes per query |
| Bench cell | `Execution::TrieLftj(SortedTrie::TreeTrie { seek, build } \| ColumnTrie { seek, build })` (`seek` carries `--ds-layout-seek`, `build` `--ds-build tree-trie=…` / `column-trie=…`) / `TrieLftj<R>` | `Execution::HashHtj { hasher, pruning, expansion, config, build }` (`build` carries `--ds-build hash-trie=…`) / `HashHtj<H, P, E>` (labels derived from `H`/`P`/`E`) |
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
    fn leaf_tuples(&self) -> Option<LeafRows<'_>>; // the chain: row ids over the tuple buffer
}
```

Because a per-level key is a hash rather than a value, `HashTrieIterable` does *not* require `IntoIterator<Item = Vec<usize>>` the way `TrieIterable` does — hash traversal is not naturally tuple-shaped. `HashTrie` exposes `collect_tuples()` for materialisation instead (a copy of its tuple buffer), and hashing means a descent can produce false positives, so the join must verify shared variables against the real tuples at the leaf, which `leaf_tuples` lends as a [`LeafRows`](#tuple-batches-tuples-and-leafrows) view.

#### TrieIteratorWrapper

Converts any `TrieIterator` into a standard Rust `Iterator<Item = Vec<usize>>` that yields complete tuples. It handles the stack management for depth-first traversal automatically. `advance()` is the allocation-free form: it lends each tuple from the wrapper's own stack, and `Iterator::next` copies that slice. `#[derive(IntoTrieIter)]` (from `kermit-derive`) generates the `IntoIterator` impl that wraps an iterator type in it. There is no hash-family equivalent, per the note above.

#### Tuple batches: `Tuples` and `LeafRows`

A relation's tuples travel from the file readers to every structure's build as one `Tuples` batch (`kermit-iters/src/tuples.rs`, #111): tuples of one arity, row-major in one `Vec<usize>`, with an explicit row count, so a batch of nullary tuples keeps its count. Each row is addressed by a `RowId` (`u32`), so a batch holds at most `u32::MAX` rows. `read_csv` and `read_parquet` return one at its exact size, and the `Catalog` (see "Catalog and lifecycle") holds it untimed; the `from_tuples*` conveniences take `impl Into<Tuples>` (test fixtures pass one `Vec<usize>` per tuple, which converts). HashTrie keeps the one copy its build makes of the rows, stores row ids into it, and threads its lists through one 4-byte link per row (`ChainLinks`, `kermit-iters/src/chain_links.rs`, #101).

A structure is built from a `TupleView<'a>` (`kermit-iters/src/tuple_view.rs`): a borrowed `&Tuples`, a `Filter` and output `columns`, where output column `i` reads source column `columns[i]`. The `Filter` is closed to the two equalities a query can express, constants (`column = value`) and repeated variables (`earlier column = later column`), kept sorted so that equal filters compare equal. A view offers `matches` and `write` for one row, `for_each` and `count` over the matching rows, and the copies `materialize` (the matching rows in source order, at exact capacity), `materialize_sorted` and `gather_sorted` (sorted output rows, one copy at every arity). `TupleView::all` is the identity view; a view with no columns is legal, and its `count()` is an all-constant atom's multiplicity. Every build mode reads a view and copies the matching rows exactly once, inside the timed build: TreeTrie `serial` and ColumnTrie read `materialize_sorted()`; TreeTrie `parallel:N` tests `matches` while scattering row ids into partitions and `write`s each worker's gathered partition; HashTrie `bulk` and `incremental` take `materialize()` as the trie's buffer; and the radix family's `partition_rows` tests `matches` in its counting pass and `write`s in its scatter pass, so the partition-ordered copy is the buffer. No build allocates per input row.

`LeafRows<'a>` (`kermit-iters/src/leaf_rows.rs`) is a `Copy` view of one hash-trie leaf chain over the buffer its row ids index, in one of two forms: *linked*, a trie's chain (its head and length, walked through the trie's `ChainLinks`), or *listed*, a slice of ids (a pruned level's one id). `HashTrieIterator::leaf_tuples` returns it. It is read by walking, never by index: `iter()`, `ids()`, or a `LeafCursor` (`first`, `advance`, `row_at`), one per relation in HashTriejoin's leaf product. The rows it lends borrow the buffer, not the view, so the leaf product reads its candidates without copying or allocating.

### Data Structures (`kermit-ds`)

#### Relation Trait

The core abstraction for relational data:

```rust
pub trait Relation: JoinIterable + Projectable {
    const KEEPS_DUPLICATES: bool = false; // HashTrie: true
    fn header(&self) -> &RelationHeader;
    fn new(header: RelationHeader) -> Self;
    fn from_view(header: RelationHeader, view: TupleView<'_>) -> Self; // required: the one build path
    fn from_tuples(header: RelationHeader, tuples: impl Into<Tuples>) -> Self; // provided: from_view over TupleView::all
    fn insert(&mut self, tuple: impl AsRef<[usize]>);
    fn insert_all(&mut self, tuples: impl Into<Tuples>);
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

struct TreeTrie<S: SeekStrategy = GallopingSeek> {
    header: RelationHeader,
    children: Vec<TrieNode>,
    tuple_count: usize, // distinct tuples; backs `Cardinality`
    _seek: PhantomData<S>,
}
```

Tuples are stored as root-to-leaf paths. Children are kept sorted so that `seek` can search them; how it searches is the seek-strategy Layout `S`, shared with `ColumnTrie` (see [`docs/data-structures/seek-strategies.md`](docs/data-structures/seek-strategies.md)).

#### ColumnTrie

A column-oriented trie inspired by the [Nemo rule engine](https://github.com/knowsys/nemo). Instead of pointer-based nodes, it uses parallel arrays:

```rust
struct ColumnTrieLayer {
    data: Vec<usize>,      // Keys at this level, sorted within intervals
    interval: Vec<usize>,  // Start indices for each parent's children
}

struct ColumnTrie<S: SeekStrategy = GallopingSeek> {
    header: RelationHeader,
    layers: Vec<ColumnTrieLayer>,
    tuple_count: usize,
    _seek: PhantomData<S>,
}
```

For a 3-ary relation, there are 3 layers. The `interval` array maps each key in layer N to the range of its children in layer N+1. This representation is more cache-friendly for large datasets. The trade-off is insertion cost: inserting a key in an early layer shifts every subsequent interval offset.

#### HashTrie

A hash-based trie, generic over a `HashStrategy` layout parameter. Each level is an open-addressing hash table keyed on the hash of that attribute's value:

```rust
struct HashTrie<H: HashStrategy = SipHashStrategy> {
    header: RelationHeader,
    tuples: Tuples,         // every stored tuple, arrival or partition order (below); len() is the multiset count
    links: ChainLinks,      // one 4-byte link per row: the next row in its list, or NIL
    root: HashTrieNode,
    _hasher: PhantomData<H>,
}

enum HashTrieNode {
    Inner(HashTable<HashTrieNode>),    // child nodes
    Leaf(HashTable<Chain>),            // chains: lists of rows in `tuples`, threaded through `links`
}
```

Tables use linear probing with power-of-two capacity, doubling above a 0.7 load factor. Two differences from the sorted tries matter:

- **Multiset semantics.** Tuples with identical hash signatures chain in the same leaf bucket rather than deduplicating, so `Cardinality::tuple_count` counts multiset size where `TreeTrie` and `ColumnTrie` count distinct tuples. `Relation::KEEPS_DUPLICATES` is `true` here, which is why an all-constant atom's count multiplies the results on `HashTrie` and not on the sorted tries.
- **Leaves reach whole tuples.** Because inner levels store only hashes, the real values are needed at the leaf to reject hash collisions. The trie keeps the one copy its build made of the view's rows, and a chain threads its tuples' row ids through the trie's links, one 4-byte link per row beside the buffer, read through `LeafRows` (#111, #101). The ids and links are 4 bytes where the paper's tuple pointers and chain pointers are 8, and the links sit in an array beside the tuples rather than inside them.
- **Construction.** `from_view` builds by the paper's Algorithm 2 (`bulk`, the default): each table's tuples are grouped into its buckets, then each bucket's child is built from its list. `bulk` and `incremental` copy the view's matching rows once (`materialize`) and keep that copy as the buffer, in source order. The partitioning BuildModes (`radix:K`, `parallel:N`, `presized:N`) copy the view's rows straight into partition order on the first attribute's hash, the paper's materialisation step (§3.3.2, #101). They do it through `morsel::partition_rows` (which tests `matches` while counting and `write`s while scattering) and keep the copy, so each partition's rows are one contiguous range, and their tries hold the same contents under other row ids. Every list is a `Chain { head, tail, len }` threaded through `links`, in input order; moving a row to the next level's list rewrites its link, as Algorithm 2 moves each tuple to exactly one child list, so a build allocates per table, never per list (#101 B2). See [`docs/data-structures/hash-trie.md`](docs/data-structures/hash-trie.md#build-modes).

`HashTrie<H, P, E>` has three Layout axes — the hasher `H` (`SipHashStrategy` default, `FxHashStrategy`), the pruning policy `P` and the expansion policy `E` — and three Config axes, the load factor, the root capacity and the child capacity; its `HasOptimizationAxes` impl reports `ds_layout_hasher`, `ds_layout_pruning`, `ds_layout_expansion`, `ds_config_load_factor`, `ds_config_root_capacity` and `ds_config_child_capacity`. `TreeTrie<S>` and `ColumnTrie<S>` implement `HasOptimizationAxes` too, reporting their seek-strategy Layout `S` (`LinearSeek`, `BinarySeek` (the default until 2026-10-05), `GallopingSeek` (the default since); see [`docs/data-structures/seek-strategies.md`](docs/data-structures/seek-strategies.md)) as `ds_layout_seek`. Each structure's BuildMode axis, `ds_build_mode` (`ColumnTrie`: `bulk` / `incremental`; `TreeTrie`: `serial` / `parallel:N`; `HashTrie`: `bulk` / `incremental` / `radix:<bits>` / `parallel:N` / `presized:N`), is reported by its bench family instead, because the built trie holds the same contents under every mode.

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
- Each result row carries exactly the head's columns, in head order; `Y` is body-only, so it is dropped, and duplicates are kept (bag semantics), so a result has one row per satisfying assignment of every body variable
- `_` in a body atom fills one column and binds nothing: it means exactly what a variable used nowhere else means

The parser checks syntax only. What a query means against the relations it runs over is checked by `kermit::db::validate_query` (see "Query validation" below).

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

Note one documented deviation from the paper: Veldhuizen keeps one persistent leapfrog per level over freely-aliased iterator arrays. Safe Rust cannot alias owned iterators across levels, so `LeapfrogTriejoinIter` instead *moves* iterators between an idle pool and the inner `LeapfrogJoinIter` on every depth change. Observable semantics are preserved; the cost is a drain/refill per `open`/`up`, which moves iterators but allocates nothing, because the one inner leapfrog is refilled in place (#83). The rationale is recorded in the module docs of `kermit-algos/src/sorted/leapfrog_triejoin.rs`.

### Hash Triejoin

`HashTriejoin` implements Algorithm 3 of the SIGMOD 2020 hash-trie-join paper, over the `HashTrieIterable` family. Unlike `LeapfrogTriejoin` it is not an iterator type — it is a recursive `enumerate` over attribute positions:

1. **Descend**: open every iterator participating at this attribute position.
2. **Choose a driver**: pick the iterator with the smallest hash table (`argmin size`) and scan it.
3. **Probe**: `lookup` that hash in every other participating iterator; recurse on a match.
4. **Emit**: at the leaf, cross-product the participating tuple chains (one `LeafCursor` per chain, an odometer whose overflow resets a cursor and carries), then **verify shared variables against the real tuple values** — hashing can produce false positives at any inner level, so equality must be re-checked before a tuple is emitted.
5. **Ascend**: `up` exactly as many times as the descent opened, including on partial-open failure.

Both algorithms stream. `JoinAlgo::join_for_each` passes each result tuple to a caller-supplied sink as a borrowed slice, so neither algorithm materialises its result: LFTJ reorders each tuple that `TrieIteratorWrapper::advance` lends into one scratch row, and HashTriejoin's leaf cross product writes into scratch buffers allocated once per join. `join_iter` is a provided method that collects through `join_for_each`, for callers that need the rows; benchmarks never call it inside a timed region (issue #65).

### JoinAlgo Trait

```rust
pub trait JoinAlgo<DS> where DS: JoinIterable {
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan,
        query: JoinQuery,
        datastructures: HashMap<String, &DS>,
        emit: S,
    );

    // Provided: collects through `join_for_each`.
    fn join_iter(
        plan: &QueryPlan,
        query: JoinQuery,
        datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> { … }
}
```

This abstraction allows implementing different join algorithms that work with any join-iterable data structure. `plan` supplies the variable descent order (see "Query Planning" below); implementations validate it against the query with `QueryPlan::validate` and panic if it is inconsistent.

The trait bound is the weak `JoinIterable` marker; each implementation narrows it to the family it can actually traverse:

```rust
impl<DS: TrieIterable>     JoinAlgo<DS> for LeapfrogTriejoin { … }
impl<DS: HashTrieIterable> JoinAlgo<DS> for HashTriejoin     { … }
```

That is what makes `(HashTrie, LeapfrogTriejoin)` a compile error rather than a runtime concern — everywhere except the CLI, which erases both sides into enums.

### Query validation

Before anything is planned, `prepare` checks the query against the catalog's headers (`kermit/src/db/validation.rs`), in a fixed order, and returns the first failure as a `JoinError`: every head term is a variable and none repeats; each body atom names a known relation, with one term per column (constants and placeholders count); every head variable occurs in the body; and every constant is `c<digits>` (checked through `extract_accesses`). There is no check on column order: the planner is free, so `edge(X, Y), edge(Y, X)` runs over two indexes of `edge`, one per column order. There is no reserved-name check either, since access ids (`@<k>`) and index ids (`#<k>`) cannot collide with a relation name. Without these checks each malformed shape took whatever path the executors' internals allowed, and the two families diverged (issue #78) — an unbound head variable panicked in LFTJ but returned no rows from Hash Triejoin, and a wrong-arity atom returned a plausible empty result.

The entry points therefore never panic on a query; they return `Result<_, JoinError>`. `validate_query(query, relations)` runs the same checks without joining, against anything implementing `RelationArities` — the `Catalog`, or a slice of `RelationHeader`s read without their tuples (`kermit_ds::read_csv_header` / `read_parquet_header`). `kermit join`, `bench join` and `bench run` (through `Workload::prepare`) use the latter, so a query that cannot run exits 1 before any relation is read in full or any measurement starts.

### Placeholder rewrite

After validation, `prepare` calls `kermit_algos::rewrite_placeholders` (see `kermit-algos/src/placeholder_rewrite.rs`), which turns every `_` in a body atom into its own fresh variable `K<i>`. Both executors assume one variable per column — the sorted iterators open one physical level per variable, and Hash Triejoin expects every iterator at its leaf once the last variable is bound — and `analyse` skips placeholders, so before the rewrite a middle `_` shifted the later columns onto the wrong level (`Q(X, Y) :- r(X, _, Y)` bound `Y` to the second column) and any `_` made Hash Triejoin panic (issue #73). After it, `_` is exactly an unused named variable, and the fresh column is dropped by the head projection.

### Access extraction

Next, `kermit_algos::extract_accesses` (see `kermit-algos/src/access.rs`) turns each body atom into an `Access { relation, filter, columns, variables }`, column by column: a variable's first occurrence in the atom becomes a kept column, a later occurrence becomes an equality with the first (`Q(X) :- r(X, X)` keeps one column and filters `col0=col1`), and a constant `c<id>` becomes a constant filter (`col1=<id>`). Kept columns stay in source order. The result is an `AccessQuery`: the query rewritten so that body atom `i` is the access id `@i` over its kept variables, the `accesses`, and the `checks`, the atoms with no kept column (only constants). This is relational algebra's selection, applied below the join as Veldhuizen 2014 §3.4 point 4 and Freitag et al. (VLDB 2020) describe, and applied while the index is built rather than during the join, so no executor, data structure or wrapper type ever sees a constant or a repeated variable. The `Filter` is closed to those two equalities because a term is a variable, a constant or `_`. Head repeats (`Q(X, X)`) are rejected by validation, and a malformed constant is the `RewriteError` that becomes `JoinError::MalformedConstant`.

### Head projection

The executors emit every variable of the executed query, in canonical order. `analyse` numbers the head's variables first, `0..head.len()`, and validation guarantees the head is distinct variables, so the head's columns are each row's prefix: `execute` lends the sink `&row[..head.len()]`, allocating nothing (issue #71). Body-only variables — written, or the fresh `K<i>` of a `_` — never reach the caller, and duplicates are kept, so row counts are unchanged.

### Query Planning

After access extraction, `prepare` gathers statistics per access id and plans the join. An unfiltered access reads its relation's catalog statistics — tuple counts always, per-column distinct counts when the optimiser's `required_statistics()` asks, both gathered once from the flat data when the `Catalog` is built. A filtered access scans the flat area through its filter for exact counts (and distinct counts when asked). A planner that asks for more than the catalog holds gets `JoinError::MissingStatistics`. These statistics go into `CatalogStats`, and the `Planner` (a `QueryOptimiser` alone; there is no column-order policy) returns a `QueryPlan` — the global attribute order the algorithm will descend. Every order is valid (`Precedence::free` has no edges); provided optimisers rank candidates inside Kahn's algorithm, and `join_for_each` asserts `QueryPlan::validate` defensively. Because the statistics come from the flat data rather than a built structure, a plan no longer depends on the structure (the sorted tries counted distinct rows where HashTrie counted duplicates), and planning is untimed and happens once per query, before any timer. `LexicographicOptimiser` (default) plans the canonical order itself, head variables first and then the body-only ones; `CardinalityOptimiser` prefers variables from small relations (`--optimiser cardinality`). `CostBasedOptimiser` (`--optimiser cost-based`) minimises the System R estimate of LFTJ's per-depth bindings by dynamic programming over the sets a valid plan can bind; see `docs/optimisers/cost-based.md`.

### Index planning

Finally, `kermit_algos::plan_indexes` (see `kermit-algos/src/index_plan.rs`) turns the plan into the indexes the query builds. Each access's kept columns are sorted by the plan position of their variables, giving an `IndexSpec { relation, filter, columns }`; identical specs merge, so a self-join that reads one relation one way builds it once, while `edge(X, Y), edge(Y, X)` builds two. Specs get ids `#0`, `#1`, … in order of first appearance, and the executed query's atoms name those ids with their variables in index-column order, so by the time the plan reaches `join_for_each` every atom's term order equals its index's column order and no index is read through a wrapper. The plan is translated by variable name, since `analyse` numbers body-only variables by first appearance. The result is an `IndexPlan { query, plan, indexes }`, which `prepare` keeps, with the checks, in a `Prepared`. The step reads only the plan and the accesses, never the data, so a symmetric relation read reversed (SNAP's undirected `edge`, #96, in the 4-cycle's `edge(D, A)`) can still build a second, identical index; writing the atom `edge(A, D)` avoids it.

## Benchmarking (`kermit-bench`)

The benchmark crate is a leaf with no internal kermit dependencies. It defines the YAML schema, performs discovery, and manages download / cache state for relation files. Criterion execution itself lives in the `kermit` binary.

### YAML benchmark definitions

A benchmark is a single `benchmarks/<name>.yml` file (filename stem must equal the `name:` field). The schema is documented in `benchmarks/README.md`. Two flavours:

- **Static** — declares `relations:` (each with a download URL or committed path, optionally pinned by a `sha256` digest) and `queries:` (Datalog strings, optionally with an `expected` result cardinality for `bench run --verify`). See `benchmarks/triangle.yml`.
- **Generator-driven** — declares a `generator: { kind: watdiv|lubm|snap|h2o|job, ... }` block instead (`scale: N` is required by `watdiv`, `lubm` and `h2o`; `snap` has none, and `job` takes `template: N` instead). The relations and queries are produced on demand by a `kermit-rdf` pipeline (`watdiv`, `lubm`) or a `kermit-generate` generator (`snap`, `h2o`, `job`).

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
```

Each query in the cache-side `benchmark.yml` carries an `expected` cardinality when the generator knows it (LUBM at scale 1, seed 0, start index 0; never WatDiv), which `bench run --verify` checks before timing.

`GeneratorSpec::spec_hash` is a SHA-256 over the canonical YAML serialisation of the spec. On `bench run <name>`, `kermit/src/materialize.rs::materialize` compares the cached `meta.json.spec_hash` against the current YAML's hash:

- **Match** → load cache-side `benchmark.yml` and short-circuit.
- **Mismatch** without `--force` → return `BenchError::SpecDrift` (regenerating a multi-minute pipeline silently is not desired).
- **Mismatch** with `--force` → wipe the cache subdir and re-run the pipeline.

Legacy `meta.json` files lacking `spec_hash` are treated as drift.

## RDF/SPARQL Pipelines (`kermit-rdf`)

`kermit-rdf` provides on-the-fly benchmark generation from RDF generators. Two pipelines:

- **WatDiv** — `pipeline::run_pipeline` drives the vendored `kermit-rdf/vendor/watdiv` binary (CLI: `-d <model> <scale>` for data, `-s ... ` for stress-template queries). The binary writes only to stdout; `driver::invoke` captures it and splits on `#end` markers. The binary is committed (vendored, ~360 KB at `bin/Release/watdiv`, force-added past the inert `**/watdiv` ignore rule); `MODEL.txt`/`files/`/`VERSION` are committed alongside it.
- **LUBM** — `lubm::pipeline::run_lubm_pipeline` drives `vendor/lubm-uba/lubm-uba.jar` (committed, ~2.9 MB), gunzips the resulting `Universities.nt.gz`, then runs Univ-Bench TBox forward chaining via `lubm::entailment` before partitioning. Requires JDK 8 on PATH. The 14 LUBM queries are committed verbatim at `kermit-rdf/queries/lubm/q1.sparql … q14.sparql` and exposed via `lubm::queries::lubm_query_specs`.

Both pipelines share post-driver stages: `partition` (split N-Triples by predicate), `parquet` (encode dict + per-predicate parquet), `dict` (string→usize), `sparql::translator` (BGP-only SPARQL → Datalog), `yaml_emit` (write the cache-side `benchmark.yml`, including each query's `expected` cardinality when the generator supplies one). The cache-side YAML always has `generator: None` — provenance lives in `meta.json.spec_hash`.

User-facing reference docs live at `docs/benchmarks/WATDIV.md` and `docs/benchmarks/LUBM.md`. Module-internal contributor notes for the LUBM driver live at `kermit-rdf/src/lubm/README.md`.

## Non-RDF Generators (`kermit-generate`)

`kermit-generate` is the layer that non-RDF generator benchmarks share; the generators themselves join it as modules: `h2o` (#95), `snap` (#96) and `job` (#97). A generator implements `Generator`: a `KIND` for `meta.json`, an `Extras` type for its own `meta.json` fields, a `target()` (output directory, benchmark name, tag, spec hash: `kermit-rdf`'s `Target`), and `generate(&mut RelationSink)`, which writes every relation and returns the queries with their `expected` counts.

- **`RelationSink`** writes `<out_dir>/<name>.parquet` through `kermit_ds::ParquetWriter`, either one-shot (`write(&header, &tuples)`) or streamed (`open(&header)` → `RelationWriter::write_batch` → `finish`), and records each relation's columns, rows and SHA-256. A writer that is dropped unfinished leaves no file and no record. `raw_dir()` gives `<out_dir>/raw/` for downloaded artifacts.
- **`run`** owns the cache contract that `materialize` and discovery rely on. It deletes any old `meta.json`, calls `generate`, checks the extras and that every query parses and names a written relation, writes `benchmark.yml`, and writes `meta.json` last. The last write is the commit marker: a directory without `meta.json` is never a cache hit.
- **`GeneratedMeta<E>`** is the common `meta.json` (schema version, kind, tag, timestamp, counts, spec hash, per-relation records) with `E` flattened beside it. It implements `kermit-rdf`'s `GeneratorMeta`, so drift detection, `OUTDATED_BEFORE` and `bench list` treat these caches like `kermit-rdf`'s.
- **`output_dir`** resolves a `bench gen <kind>` output directory (`--output-dir` or the cache root) and reports whether discovery will see it.

`job` is the one generator that drives an external tool. Its translator parses JOB's SQL with `sqlparser` into relation files and Datalog rules; DuckDB (`KERMIT_DUCKDB_BIN`, else `PATH`) then applies each relation's filters, streaming the rows into a `RelationWriter`, and computes each query's `expected` as `COUNT(*)`, over a raw IMDb cache (`<cache>/kermit/imdb/`) built once and shared by every template. See `docs/benchmarks/JOB.md`.

## CLI (`kermit`)

The binary exposes two top-level subcommands:

- `kermit join` — execute a Datalog query against relation files and print the result tuples. It reads the relations the query names into a `Catalog`, then prepares, builds and executes.
- `kermit bench` — Criterion-driven benchmarking, with the following subcommands:
  - `bench join` — wrap a join in Criterion timing.
  - `bench ds` — measure build / iteration / heap size for a single relation file against one or more index structures.
  - `bench run [<NAME> | --all]` — run YAML-defined benchmarks; `--metrics` selects from `build`, `iteration`, `space`. `--force` opts into regenerating a generator-driven benchmark when its spec hash drifts.
  - `bench list` — list benchmarks; status distinguishes `cached` / `not cached` for static and `not generated` / `cached` / `stale` for generator-driven.
  - `bench fetch [<NAME>]` — download relation parquets for static benchmarks.
  - `bench clean [<NAME>]` — remove cached benchmark artefacts.
  - `bench gen { watdiv | lubm | snap | h2o | job }` — imperative on-the-fly generation, bypassing YAML; writes into the same cache layout under a user-supplied `--tag`.

Working examples live in `README.md` and `USAGE.md`; the YAML schema and generator-spec details live in `benchmarks/README.md`.

### Catalog and lifecycle

`kermit/src/db.rs` runs a query as four steps over a `Catalog` (`kermit/src/db/catalog.rs`): the relations keyed by header name, each a header and a flat `Tuples` batch with no structure built, plus the statistics gathered once when it is built, up to the `StatisticsLevel` its optimiser declares (tuple counts are row counts; column-distinct counts only at `ColumnDistinct`). A later relation with a duplicate name replaces the earlier one.

- **Load** (untimed) reads each relation the queries name into the catalog.
- **`prepare(&catalog, &query, &planner) -> Result<Prepared, JoinError>`** (untimed, once per query; `kermit/src/db/lifecycle.rs`) validates, rewrites `_`, extracts accesses, gathers statistics, plans, and plans indexes. A `Prepared` holds the executed query, the plan, the `IndexSpec`s, the constant checks and the head's length.
- **`build::<R>(&catalog, &prepared) -> Indexes<R>`** (timed) turns each spec into a `TupleView` over its relation's flat tuples and calls the structure's `from_view`, which copies the matching rows once; each constant check becomes its view's `count()`. A zero count means the query has no answers; otherwise the counts multiply into `Indexes::multiplicity()`, each clamped to 1 unless `R::KEEPS_DUPLICATES`. `build_with` takes the constructor as a closure, which is how the binary's families inject their config and build mode.
- **`execute::<R, JA>(&prepared, &indexes, emit)`** (timed) maps ids to indexes, calls `JoinAlgo::join_for_each`, and lends `emit` each row's head prefix once per unit of multiplicity, without materialising the result; when every atom was a constant check it emits the empty head row `multiplicity` times and runs no join. Executing with indexes built from another `Prepared` is a programming error, caught by a debug assertion that the specs are equal.

The conveniences `lftj_join_for_each<R, JA>` and `hash_join_for_each<R>` (hardwired to `HashTriejoin`) run all three steps and stream; `lftj_join` / `hash_join` collect through them and return `Result<Vec<Vec<usize>>, JoinError>`; a query that cannot run returns a `JoinError` before anything is emitted. `kermit::compute_join` builds a `Catalog` from its inputs and calls `prepare`, `build` and `execute` itself (it is generic over the relation and the algorithm, which the family-bound conveniences are not), so it validates and applies every rewrite like every other entry point. Freeing what a timed step built is never timed: each step returns what it built, and Criterion drops it after the timer stops.

In the binary, `RelationFamily::build_index(header, view)` is each family's one config- and mode-aware construction site. `ExecutionFamily::build_indexes(&catalog, &prepared)` is `build_with` over it, `execute(&prepared, &indexes, sink)` runs the join, and `count_rows` counts a join's rows through `black_box`. The families hold no `Planner`, because `prepare` runs before them. `bench run` reads every relation its queries name into one `Catalog` before the loop over cells (`Workload::prepare`, which returns a `PreparedWorkload`), prepares each query once, and shares both across every cell; `bench join` (both `--output` and the benchmark) and `kermit join` go through the same `Workload::prepare`.

There is deliberately no object-safe engine trait. Runtime selection of the `(structure, algorithm)` cell — for `kermit join` and `bench join` as much as for `bench run` — goes through `Execution::for_pair` and the `ExecutionFamily` impls in `kermit/src/execution.rs` (`query_cell` in `main.rs` resolves the one cell both join commands use, and `load_query_runner` monomorphises it exactly as `dispatch_run_bench` does; `build_join_runner` debug-asserts that the family it was handed reports that cell). An incompatible pair is a usage error, and all three valid cells, including `(HashTrie, HashTriejoin)`, are reachable from every command.

### Selector dispatch

`bench ds` and `bench run` accept `all` for `--indexstructure` and `--algorithm`, expanding to a Cartesian sweep.

For `bench run`, that sweep is expressed as *cells* rather than pairs. `kermit/src/execution.rs` defines `Execution`, an enum whose variants each fix **both** halves of the combination — `TrieLftj(SortedTrie::TreeTrie { seek, build } | ColumnTrie { seek, build })` (`seek` carries the `--ds-layout-seek` strategy, and `build` the structure's `--ds-build` mode) and `HashHtj { hasher, pruning, config }` — so an `Execution` cannot describe something the CLI is unable to run. `Execution::for_pair` is the sole constructor and returns `None` for the three incompatible pairs; `Sweep::expand` partitions the cross product into `cells` and a `skipped` list.

Consequently `-i all -a all` runs exactly the three valid cells, announcing each skipped pair on stderr, while a single explicitly-named incompatible pair leaves nothing to run and is reported as a usage error. Because the report's `data_structure` and `algorithm` axes are both derived from `ExecutionFamily::execution()`, a report cannot name an algorithm it did not run (issue #56).

`bench ds` selects a structure but no algorithm, so it names its cell through the total `Execution::for_structure` (every structure has exactly one compatible algorithm) and runs the same kind of generic runner, `run_ds_bench<F: RelationFamily>`. `RelationFamily` is the relation-facing supertrait of `ExecutionFamily` (index construction, axes, cell label); `bench ds` instantiates the structure-only markers `SortedTrieFamily<R>` / `HashTrieFamily<H, P>`, which implement the supertrait alone and so cannot join. The join families `TrieLftj<R>` / `HashHtj<H, P>` embed those markers and delegate, so the two commands cannot disagree about a structure (issue #61). `HashTrieFamily<H, P>` is also where the `--ds-config` values live, so the `build` and `end_to_end` closures build through `RelationFamily::build_index` and measure the structure the report's `ds_config_*` axes describe. `SortedTrieFamily<R>` likewise carries the `--ds-build` mode (`R::BuildMode`: `ColumnTrieBuildMode` or `TreeTrieBuildMode`), builds through it, and reports it as `ds_build_mode` via `RelationFamily::build_mode_axes`.

### Space measurement

`kermit/src/measurement.rs` defines a custom Criterion `Measurement` (`SpaceMeasurement`) plus a `BytesFormatter` that picks a binary-prefixed unit (`B`/`KiB`/`MiB`/`GiB`). When `--metrics space` is requested, both `bench ds` and `bench run` route through `Criterion<SpaceMeasurement>` via `iter_custom`, producing `target/criterion/{group}/{dir}/...` JSON alongside the time metrics. The per-DS hook is the `HeapSize` trait (`heap_size_bytes()`); `add_space_bench` takes a slice of built relations and the closure sums their `heap_size_bytes()` once per iter (one relation for `bench ds`; a query's indexes together for `bench run`, one `space` function per query).

### JSON bench reports

Every `kermit bench` invocation writes a `BenchReport` JSON array to disk. The default path is `bench-runs/{kind}-{unix-millis}.json` (the directory is auto-created and gitignored at the workspace root); pass `--report-json <PATH>` to override. Each report carries `metadata` (label/value pairs mirroring stderr), `axes` (a structured map for tooling: `data_structure`, `algorithm`, `query`, `tuples`, …), and `criterion_groups` pointers resolving to per-function `target/criterion/{group}/{dir}/` artefacts. The schema is versioned by `schema_version` (currently `5`) and lives in `kermit/src/bench_report.rs`; the full key catalogue is documented in `docs/specs/bench-report-schema.md`. Since v5 the join metrics are per query (`build` replaces `insertion` and `copies`; `iteration`, `end_to_end` and `space` follow), the `column_orders` axis is gone, an `index_rows` axis holds the rows the build copied, and each index adds an `index` metadata line (id, relation, filter, column order, rows, bytes).

## Analysis (`python/kermit-lab`)

`python/kermit-lab/` is a uv-managed Python package for notebook-first analysis of benchmark output. It is not a Cargo workspace member and shares no code with the Rust side — **the only coupling is the on-disk data contract**:

- `kl.load("bench-runs/*.json")` parses `BenchReport` arrays into a pandas `DataFrame`, including the `ds_*` / `algo_*` optimization axes. It checks `schema_version` on load and raises `SchemaError` if a report is newer than the package understands.
- Each `criterion_groups` pointer is then resolved to its `target/criterion/<group dir>/<function dir>/new/*.json` through a `CriterionIndex` built once per load, keyed on every `new/benchmark.json`'s `(group_id, function_id)` — never by recomputing the directory names, which Criterion escapes, truncates to 64 bytes and de-duplicates with `_2` suffixes.
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
2. Implement `Relation` + `Projectable` + `HeapSize` + `Cardinality` on the structure. `HeapSize::heap_size_bytes` returns heap bytes only, excluding `size_of::<Self>`. `Relation::from_view` is the required constructor: it reads a `TupleView` and copies the matching rows once, and the `from_tuples*` constructors are provided over `TupleView::all`; `KEEPS_DUPLICATES` is `true` if the structure counts duplicate rows.
3. Implement the iterator. For the sorted family that is `TrieIterator` plus `#[derive(IntoTrieIter)]`, then `TrieIterable` on the structure. For the hash family it is `HashTrieIterator` and `HashTrieIterable` — and note that the shared helpers `project_via_trie_iter` and `TrieIteratorWrapper` are `TrieIterable`-only, so a hash-family structure must supply its own equivalents. The `kermit-ds` test macros fork along the same seam: `hash_trie_test_suite!` covers the hash family, `relation_trie_test_suite!` the sorted one.
4. Register the module and add a variant to `IndexStructure` in `kermit-ds/src/ds/mod.rs`.
5. Wire the CLI: a variant on `IndexStructureSelector` and its `expand()`. A sorted-family structure also needs a `SortedTrie` variant plus a `SortedTrieRelation` impl (with its `BuildMode`, `()` for a single build process) and an `Execution::for_pair` arm (`kermit/src/execution.rs`), an arm in `load_query_runner` (`kermit/src/main.rs`), and arms in `dispatch_run_bench` / `dispatch_ds_bench` (`kermit/src/bench/{run,ds}.rs`). A structure in a new trait family needs its own `ExecutionFamily` impl.
6. **Wire the tests.** Add `define_multiway_join_test_suite!(<Type>, <Algo>, LexicographicOptimiser)` and a second invocation with `CardinalityOptimiser` in `kermit/tests/join_tests.rs`, so all 16 standard join patterns run against the structure. Each distinct layout combination is its own suite invocation (e.g. `HashTrieSip`, `HashTrieFx`). Add a cell for the structure in `kermit/tests/all_orders.rs`, which runs every query shape under every variable order, and in `kermit/tests/result_allocation.rs` and `kermit/tests/tuple_allocation.rs`.
7. **Write the doc** at `docs/data-structures/<name>.md` from the template.

### New Join Algorithm

1. Create `kermit-algos/src/<family>/<name>.rs` (`sorted/` or `hash/`, matching the iterator family) and implement `JoinAlgo<DS>`, narrowing `DS` to `TrieIterable` or `HashTrieIterable` accordingly. The implementation runs over the executed query (every atom an index id `#<k>` over a plain index, each variable once per atom, columns in plan order, no constants) and must validate the incoming `QueryPlan`.
2. Register the module in the family's `mod.rs`, re-export it from the flat facade in `kermit-algos/src/lib.rs`, and add a variant to the `JoinAlgorithm` enum there.
3. Wire the CLI: a variant on `JoinAlgorithmSelector` and its `expand()`, plus the algorithm's valid cells in `Execution` / `Execution::for_pair` (the compiler flags the incomplete match) and a `dispatch_run_bench` arm. `kermit join` / `bench join` dispatch through the same cells (`load_query_runner`), so nothing extra is needed for them. A sorted-family algorithm plugs into `lftj_join<R, JA>`; a hash-family one needs its own entry point beside `hash_join` over `prepare` / `build` / `execute` (see "Catalog and lifecycle").
4. **Wire the tests.** Add a `define_multiway_join_test_suite!` invocation per compatible index structure × optimiser, and a cell in `kermit/tests/all_orders.rs`.
5. **Write the doc** at `docs/algorithms/<name>.md` from the template.

### New Query Optimiser

1. Create `kermit-algos/src/optimiser/<name>.rs` and implement `QueryOptimiser`.
2. Build the ordering with the shared `topological_order(&Precedence::free(num_vars), rank)`. With no edges every variable is Kahn-ready, so any rank function yields a valid plan — a policy can be slow, never wrong — and the ranking alone decides. Consume statistics via `CatalogStats`, treating a missing entry as "assume large". Override `required_statistics()` if the policy reads more than tuple counts.
3. Register the module and add a variant to the `Optimiser` enum, with `instantiate()` and `axis_value()` arms. The `axis_values_match_clap_value_names` guard test pins axis naming.
4. **Wire the tests.** A `define_multiway_join_test_suite!` invocation per valid (structure, algorithm) pair, plus unit tests for the ranking itself.
5. **Write the doc** at `docs/optimisers/<name>.md` from the template.

### New Benchmark

1. Create `benchmarks/<name>.yml`. The schema is documented in `benchmarks/README.md`.
   - For a static benchmark, declare `relations:` (with download URLs) and `queries:` (Datalog strings).
   - For a generator-driven benchmark, declare `generator: { kind: watdiv | watdiv-basic | lubm | snap | h2o | job, ... }` instead.
2. Run `kermit bench list` to verify discovery and validate the YAML.
3. For static benchmarks, `kermit bench fetch <name>` downloads the relation parquets. For generator-driven benchmarks, `kermit bench run <name>` materialises them on first invocation.
