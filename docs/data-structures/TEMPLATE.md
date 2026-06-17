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

- One bullet per invariant maintained by `Relation::insert` / `from_tuples` / `insert_all`. Include the consequence — what breaks if the invariant breaks.
- Include LFTJ-related invariants explicitly: the `open`-after-`at_end` discipline (see the LFTJ gotcha in `CLAUDE.md`), forward-only `seek`, sorted siblings.

## Complexity

Let `n` = tuple count, `a` = arity, `b` = average branching factor.

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | O(…) | O(…) | |
| `from_tuples(n)` | O(…) | O(…) | |
| `TrieIterator::key()` | O(…) | | |
| `TrieIterator::next()` | O(…) | | |
| `TrieIterator::seek(target)` | O(…) | | |
| `TrieIterator::open()` | O(…) | | |
| `TrieIterator::up()` | O(…) | | |
| `HeapSize::heap_size_bytes()` | O(…) | | |

The `TrieIterator::*` rows fit the sorted tries (`TreeTrie`, `ColumnTrie`). Hash-based structures expose the separate `HashTrieIterator` trait instead — document `HashTrieIterator::key/next/lookup/size/open/up/leaf_tuples` (no `seek`; hash navigation is exact-match) in their place (see `docs/data-structures/hash-trie.md`).

Call out any known suboptimal asymptotic (e.g. linear-scan where a binary search is possible) so a future reader can tell intent from oversight.

## Worked micro-example

Build the structure from a handful of tuples — three or four is enough. Show the in-memory layout (data arrays, child pointers, intervals, etc.) and one iteration walk that exercises both `open` and `up`.

## When to prefer this structure

A short paragraph. Workload shapes where this structure shines vs. siblings. Workload shapes where a sibling is the better choice.

## See also

- Sibling docs in `docs/data-structures/`.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; this structure must pass all 11 patterns under every compatible algorithm (Priorities item 1).
- Algorithm doc(s) that consume the `TrieIterable` contract.
