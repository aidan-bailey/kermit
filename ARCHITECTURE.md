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
├── kermit-bench/    # Benchmark infrastructure
└── kermit/          # CLI and top-level integration
```

### Dependency Graph

```
kermit-iters ◄─── kermit-derive
     │
     ├──────────── kermit-parser
     │                  │
     ▼                  ▼
kermit-ds ◄─────── kermit-algos
     │                  │
     └────────┬─────────┘
              ▼
        kermit-bench
              │
              ▼
           kermit
```

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
pub trait JoinAlgo<DS> where DS: TrieIterable {
    fn join_iter(
        query: JoinQuery,
        datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>>;
}
```

This abstraction allows implementing different join algorithms that work with any trie-iterable data structure.

## Benchmarking (`kermit-bench`)

The benchmark crate provides synthetic data generation and workload definitions
with no internal kermit dependencies:

- **Generation**: Tuple generators (exponential, factorial, distinct) and graph
  model stubs (Erdos-Renyi via petgraph)
- **Tasks**: Groupings of related benchmark workloads
- **SubTasks**: Individual scale points with declarative generation parameters

```rust
enum GenerationParams {
    Exponential { k: usize },
    Factorial { k: usize },
    Graph(GraphModel),
    Custom,
}

trait BenchmarkConfig {
    fn metadata(&self) -> &BenchmarkMetadata;
    fn generate(&self, subtask: &SubTask) -> Vec<(usize, Vec<Vec<usize>>)>;
}
```

## CLI (`kermit`)

The binary provides two main commands:

```bash
# Execute a join query
kermit join \
  --relations data1.csv data2.csv \
  --query query.txt \
  --algorithm leapfrog-triejoin \
  --indexstructure tree-trie

# Run a named benchmark suite on synthetic data
kermit bench suite \
  --benchmark exponential \
  --indexstructure tree-trie \
  --metrics insertion iteration space

# Benchmark a single data structure on a file
kermit bench ds \
  --relation data.csv \
  --indexstructure column-trie
```

## File I/O

Relations can be loaded from:
- **CSV**: Header row defines attribute names, filename becomes relation name
- **Parquet**: Schema provides attribute names, efficient columnar storage

The `RelationFileExt` trait provides `from_csv()` and `from_parquet()` methods via blanket implementation for any `Relation`.

## Key Type

All keys are `usize`. String values must be dictionary-encoded before use. This simplifies the implementation and improves performance for join comparisons.

## Adding New Components

### New Data Structure

1. Implement `Relation` trait in `kermit-ds`
2. Implement `TrieIterable` (which requires `JoinIterable`)
3. Create an iterator type implementing `TrieIterator`
4. Add to `IndexStructure` enum for CLI selection

### New Join Algorithm

1. Implement `JoinAlgo<DS>` trait in `kermit-algos`
2. Add to `JoinAlgorithm` enum for CLI selection
3. The algorithm receives a `JoinQuery` and map of data structures

### New Benchmark

1. Create a module in `kermit-bench/src/benchmarks/`
2. Define `BenchmarkMetadata` with tasks and subtasks using `GenerationParams`
3. Implement `BenchmarkConfig` trait (generate method)
4. Add to `Benchmark` enum
