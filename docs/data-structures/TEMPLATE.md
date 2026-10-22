# `<StructureName>`

> **Status:** [stable | experimental] · **CLI:** `-i <structure-name>` · **Implementation:** [`kermit_ds::ds::<module>`](../../kermit-ds/src/ds/<module>/)

## Representation

Diagram or pseudo-Rust showing the in-memory layout. Mention what is heap-allocated, what is inline, and how depth/levels are tracked. If the structure pairs with a custom iterator type, describe how iterator state encodes a trie position.

```rust
StructureName {
    header: RelationHeader,
    // …
}
```

## Invariants

- One bullet per invariant maintained by `Relation::insert` / `from_view` / `insert_all`. Include the consequence — what breaks if the invariant breaks.
- Include LFTJ-related invariants explicitly: the `open`-after-`at_end` discipline (see the LFTJ gotcha in `CLAUDE.md`), forward-only `seek`, sorted siblings.
- State whether the structure keeps duplicate rows (`Relation::KEEPS_DUPLICATES`): a constant-only atom's row count multiplies a join's results only where it does.

## Complexity

Let `n` = tuple count, `a` = arity, `b` = average branching factor.

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | O(…) | O(…) | |
| `from_view(view)` | O(…) | O(…) | |
| `TrieIterator::key()` | O(…) | | |
| `TrieIterator::next()` | O(…) | | |
| `TrieIterator::seek(target)` | O(…) | | |
| `TrieIterator::open()` | O(…) | | |
| `TrieIterator::up()` | O(…) | | |
| `HeapSize::heap_size_bytes()` | O(…) | | |

The `TrieIterator::*` rows fit the sorted tries (`TreeTrie`, `ColumnTrie`). Hash-based structures expose the separate `HashTrieIterator` trait instead — document `HashTrieIterator::key/next/lookup/size/open/up/leaf_tuples` (no `seek`; hash navigation is exact-match) in their place (see `docs/data-structures/hash-trie.md`).

Call out any known suboptimal asymptotic (e.g. linear-scan where a binary search is possible) so a future reader can tell intent from oversight.

## Construction

How `from_view(header, view)` reads its `TupleView`, and the one copy it makes:
- which view methods the build calls (`materialize`, `materialize_sorted`, `gather_sorted`, or `matches` and `write` while partitioning);
- what the copy is (scratch freed at the end of the build, or the structure's own buffer) and its capacity (exact: the build allocates it at the size of the matching rows);
- what each BuildMode copies, so that every mode builds an equivalent structure from the same view (the same contents and capacities, hence the same `heap_size_bytes`).

A build never moves or frees the view's source (the catalog's flat area) and allocates nothing per input row.

## Worked micro-example

Build the structure from a handful of tuples — three or four is enough. Show the in-memory layout (data arrays, child pointers, intervals, etc.) and one iteration walk that exercises both `open` and `up`.

## When to prefer this structure

A short paragraph. Workload shapes where this structure shines vs. siblings. Workload shapes where a sibling is the better choice.

## See also

- Sibling docs in `docs/data-structures/`.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; this structure must pass all 16 patterns under every compatible algorithm (Priorities item 1).
- Algorithm doc(s) that consume the `TrieIterable` contract.
