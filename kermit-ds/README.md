# kermit-ds

Relation data structures for the Kermit workspace. Provides four implementations that store tuples of `usize` keys, three tries and a hash index:

- [`TreeTrie`](src/ds/tree_trie/implementation.rs) — a pointer-based trie where each node owns its sorted children. Simple and direct; preferable for small relations or pedagogical use.
- [`ColumnTrie`](src/ds/column_trie/implementation.rs) — a column-oriented (flattened) trie that stores each depth in parallel `data`/`interval` arrays. More compact and cache-friendly on large relations.
- [`HashTrie`](src/ds/hash_trie/implementation.rs) — a hash-based trie storing the relation as nested hash tables, generic over a `HashStrategy` (default `SipHashStrategy`).
- [`HashIndex`](src/ds/hash_index/implementation.rs) — a chained hash table keyed on the first `levels()` columns of the relation, the structure of the binary hash join baseline; it stores the rows once, beside a power-of-two directory of bucket heads and one link per row. Generic over a `HashStrategy` (default `SipHashStrategy`).

`TreeTrie` and `ColumnTrie` implement [`Relation`](src/relation.rs) and [`TrieIterable`](../kermit-iters/src/trie.rs), so they're interchangeable under `LeapfrogTriejoin` in [`kermit-algos`](../kermit-algos). `HashTrie` implements `Relation` and [`HashTrieIterable`](../kermit-iters/src/hash_trie.rs) instead, so it is driven by `HashTriejoin` rather than the LFTJ path. `HashIndex` implements `Relation` and [`KeyedIterable`](../kermit-iters/src/keyed.rs) (a third, parallel family: `scan` and an exact-match `probe` on a key prefix, with no trie levels), so it is driven by `BinaryHashJoin`.

## Surface

- **Core traits** — `Relation`, `Projectable`, `RelationFileExt`, `HeapSize`.
- **Metadata** — `RelationHeader`, `ModelType`, `RelationError`.
- **Data structures** — `TreeTrie`, `ColumnTrie`, `HashTrie`, `HashIndex`, plus the `IndexStructure` CLI enum.

## File loading

Any `Relation` automatically gains `from_csv` and `from_parquet` via the blanket `RelationFileExt` impl. CSV files must be `usize`-valued with a header row; Parquet files must be `Int64`-valued. Both extract the relation name from the file stem.

```rust,ignore
use kermit_ds::{RelationFileExt, TreeTrie};

let edges = TreeTrie::from_csv("edges.csv")?;
```

## Extending

To add a new data structure:

1. Implement `Relation + Projectable + HeapSize` in this crate. `Relation::from_view(header, TupleView)` is the one required bulk constructor (it reads the view and copies its matching rows once); `from_tuples` is provided over it. Also implement an iterator trait: `TrieIterable` for a hierarchical (LFTJ-style) trie, `HashTrieIterable` for a hash-based trie (see `HashTrie`), or `KeyedIterable` for an index keyed on its leading columns (see `HashIndex`).
2. Add a variant to `IndexStructure` in [`src/ds/mod.rs`](src/ds/mod.rs).
3. Wire it into the CLI dispatch (`run_ds_bench` / `run_benchmark` in [`../kermit/src/main.rs`](../kermit/src/main.rs)).

See [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the design rationale behind the trie layouts.
