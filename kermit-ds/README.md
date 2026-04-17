# kermit-ds

Relation data structures for the Kermit workspace. Provides two trie-based implementations that store tuples of `usize` keys:

- [`TreeTrie`](src/ds/tree_trie/implementation.rs) — a pointer-based trie where each node owns its sorted children. Simple and direct; preferable for small relations or pedagogical use.
- [`ColumnTrie`](src/ds/column_trie/implementation.rs) — a column-oriented (flattened) trie that stores each depth in parallel `data`/`interval` arrays. More compact and cache-friendly on large relations.

Both implement [`Relation`](src/relation.rs) and [`TrieIterable`](../kermit-iters/src/trie.rs), so they're interchangeable in the join algorithms in [`kermit-algos`](../kermit-algos).

## Surface

- **Core traits** — `Relation`, `Projectable`, `RelationFileExt`, `HeapSize`.
- **Metadata** — `RelationHeader`, `ModelType`, `RelationError`.
- **Data structures** — `TreeTrie`, `ColumnTrie`, plus the `IndexStructure` CLI enum.

## File loading

Any `Relation` automatically gains `from_csv` and `from_parquet` via the blanket `RelationFileExt` impl. CSV files must be `usize`-valued with a header row; Parquet files must be `Int64`-valued. Both extract the relation name from the file stem.

```rust,ignore
use kermit_ds::{RelationFileExt, TreeTrie};

let edges = TreeTrie::from_csv("edges.csv")?;
```

## Extending

To add a new data structure:

1. Implement `Relation + TrieIterable + HeapSize` in this crate.
2. Add a variant to `IndexStructure` in [`src/ds/mod.rs`](src/ds/mod.rs).
3. Wire it into the CLI dispatch (`run_ds_bench` / `run_benchmark` in [`../kermit/src/main.rs`](../kermit/src/main.rs)).

See [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the design rationale behind the trie layouts.
