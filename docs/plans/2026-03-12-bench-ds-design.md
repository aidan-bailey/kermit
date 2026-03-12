# Design: `bench ds` Subcommand

## Overview

Add a `bench ds` sub-subcommand to benchmark index structures (data structures) in isolation, measuring insertion time, iteration time, and heap space. The existing `bench` command becomes `bench join`.

## CLI Structure

```
kermit bench join -r R1.csv -r R2.csv -q query.dl -a leapfrog-triejoin -i tree-trie
kermit bench ds   -r relation.csv -i tree-trie --metrics insertion iteration space
```

`Commands::Bench` wraps shared Criterion args (`BenchArgs`) and a `BenchSubcommand` enum:

```rust
Commands::Bench {
    #[command(flatten)]
    bench_args: BenchArgs,        // sample-size, measurement-time, warm-up-time, name
    #[command(subcommand)]
    subcommand: BenchSubcommand,  // Join { ... } | Ds { ... }
}
```

`BenchSubcommand::Join` — current `QueryArgs` + `--output` (unchanged behavior).

`BenchSubcommand::Ds`:
- `--relation / -r` — single `PathBuf` (required)
- `--indexstructure / -i` — `IndexStructure` enum (required)
- `--metrics / -m` — one or more of `insertion`, `iteration`, `space` (defaults to all three)

This is a breaking CLI change: `kermit bench ...` becomes `kermit bench join ...`.

## Metrics Enum

```rust
#[derive(Copy, Clone, Debug, ValueEnum)]
enum Metric {
    Insertion,
    Iteration,
    Space,
}
```

Accepted via `--metrics` / `-m` with `num_args = 1..`, defaulting to all three.

## What Each Metric Measures

**Insertion** — Times `R::from_tuples(header, tuples)` via Criterion `iter_batched`. Setup clones the pre-loaded tuples and header. Captures full construction cost (sorting + insertion).

**Iteration** — Times `trie.trie_iter().into_iter().collect::<Vec<_>>()`. The relation is pre-built and shared immutably across iterations. Measures full depth-first trie traversal via `TrieIteratorWrapper`.

**Space** — Not a Criterion benchmark. Calls `HeapSize::heap_size_bytes()` on the constructed relation and prints the result to stderr. Single measurement, no statistical sampling.

Criterion bench IDs: `{IndexStructure}/{metric}`, e.g. `TreeTrie/insertion`, `TreeTrie/iteration`.

Metadata printed to stderr:
```
--- bench ds metadata ---
  data structure:  TreeTrie
  relation:        R.csv
  tuples:          50000
  arity:           3
  heap bytes:      1234567
```

## HeapSize Trait

In `kermit-ds`:

```rust
pub trait HeapSize {
    fn heap_size_bytes(&self) -> usize;
}
```

Counts heap-allocated bytes only (Vec backing buffers), not stack size of the struct.

**TreeTrie**: root `Vec<TrieNode>` capacity + recursively each node's `children: Vec<TrieNode>` capacity.

**ColumnTrie**: each `ColumnTrieLayer`'s `data` and `interval` Vec capacities * `size_of::<usize>()`, plus the `layers: Vec<ColumnTrieLayer>` capacity.

## Execution Flow

1. Destructure `BenchArgs` + `BenchSubcommand`.
2. Build `Criterion` from shared args.
3. Match on subcommand:

**`BenchSubcommand::Join`** — identical to current `bench` logic.

**`BenchSubcommand::Ds`**:
1. Load relation file, construct the relation, iterate it back to get raw tuples for the insertion benchmark setup closure.
2. Print metadata to stderr.
3. For each selected metric:
   - **Insertion**: `iter_batched` cloning tuples+header, timing `R::from_tuples`.
   - **Iteration**: `iter` timing full trie iteration on the pre-built relation.
   - **Space**: print `heap_size_bytes()` to stderr.
4. `group.finish()`, `criterion.final_summary()`.

Monomorphisation boundary: match on `IndexStructure` calling a generic inner function with the concrete type (same pattern as `instantiate_database`).
