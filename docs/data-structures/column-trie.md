# `ColumnTrie`

> **Status:** stable · **CLI:** `-i column-trie` · **Implementation:** [`kermit_ds::ds::column_trie`](../../kermit-ds/src/ds/column_trie/)

## Representation

`ColumnTrie` flattens each trie level into a `ColumnTrieLayer` of parallel arrays: `data: Vec<usize>` for keys at that depth, and `interval: Vec<usize>` for parent → child offsets. One layer per attribute. The children of the parent at position `i` span `data[interval[i]..interval[i+1]]` of the *child* layer (or to `data.len()` for the last parent).

```rust
ColumnTrie {
    header: RelationHeader,
    layers: Vec<ColumnTrieLayer>,    // one per attribute
}

ColumnTrieLayer {
    data: Vec<usize>,                // sorted within each sibling interval
    interval: Vec<usize>,            // parent index → start offset in data
}
```

The iterator `ColumnTrieIter` carries a triple position `(depth, interval_i, rel_data_i)`:

- `depth` — `0` = root/uninitialised; `1..=arity` selects a data layer.
- `interval_i` — index into the current layer's `interval` array, identifying *which parent element's children* we are scanning.
- `rel_data_i` — offset within the active sibling slice (relative, not a global index).

`open()` derives the new layer's `interval_i` as `parent_start + rel_data_i`; `up()` reverses this by searching the parent's interval array for the interval that owns the current global data index.

Compared to [`TreeTrie`](./tree-trie.md), this layout avoids per-node allocation and is cache-friendly for large relations, at the cost of expensive inserts — placing a key in an early layer can force later-layer offsets to shift.

## Invariants

- **`layers.len() == header.arity()`.**
- **Sorted within sibling intervals.** Each layer's `data` is sorted ascending within every `data[interval[i]..interval[i+1]]` span. Keys belonging to different parents are *not* globally sorted across the entire `data` array.
- **Interval-length coherence.** `layers[i].interval.len()` equals the number of distinct keys in `layers[i-1].data` (1 for the root layer when non-empty, 0 when empty).
- **Interval bounds.** Every interval entry indexes into `layers[i].data`; the last entry may equal `data.len()`.
- **Duplicate insert is a no-op.** When an inserted key matches an existing key in the active interval, insertion *recurses into the existing key's position* — the next layer is inserted under the right parent. Encoded as [`LayerStep::Recurse`](../../kermit-ds/src/ds/column_trie/implementation.rs#L218).
- **Forward-only `seek`.** Like `TreeTrieIter`, `ColumnTrieIter::seek(k)` only advances; it uses `partition_point` on the remaining slice.
- **LFTJ `open`-after-`at_end` discipline.** `ColumnTrieIter::open` derives the new interval index from `parent_start + rel_data_i` even if `rel_data_i` is currently past the end of the parent's children. LFTJ relies on this — do not "fix" the apparent stale offset. See the LFTJ gotcha in `CLAUDE.md`.

## Complexity

Let `n` = tuple count, `a` = arity, `b` = average branching factor.

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | O(a · b) worst-case | O(1) extra | linear search within interval; insertions in early layers shift later-layer offsets (`insert_key_and_shift_intervals`) |
| `from_tuples(n)` | O(n · a · b + n · a · log n) | O(n · a) | sort lexicographically, then insert |
| `TrieIterator::key()` | O(1) | | slice index |
| `TrieIterator::next()` | O(1) | | `rel_data_i += 1` |
| `TrieIterator::seek(target)` | O(log b) | | `partition_point` binary search on the sorted sibling slice |
| `TrieIterator::open()` | O(1) | | derive child interval and slice |
| `TrieIterator::up()` | O(parent interval length) | | linear scan of parent's interval array |
| `HeapSize::heap_size_bytes()` | O(a) | | sum of `Vec` capacities per layer |

`up()` is suboptimal — a binary search on the sorted `interval` array would make it `O(log)`. Acceptable today because LFTJ ascends one level at a time and parent layers are typically short relative to leaf layers, but a known suboptimality worth noting for very wide tries.

## Worked micro-example

Tuples `{(1, 2), (1, 3), (2, 4)}` produce:

```
layer 0 (a):
    data     = [1, 2]
    interval = [0]            # one root interval

layer 1 (b):
    data     = [2, 3, 4]
    interval = [0, 2]         # children of `1` are data[0..2] = [2, 3]
                              # children of `2` are data[2..3] = [4]
```

Iteration walk:

1. `open()` (root → depth 1): `rel_data = layer0.child_data(0) = [1, 2]`. Current = `1`.
2. `open()` (depth 1 → depth 2): `parent_start = layer0.interval[0] = 0`, `interval_i = 0 + 0 = 0`. `rel_data = layer1.child_data(0) = [2, 3]`. Current = `2`. Emit `[1, 2]`.
3. `next()` → current = `3`. Emit `[1, 3]`.
4. `next()` → `at_end()` at depth 2. `up()` → search `layer0.intervals = [0]` for owner of `data_index = 0`; pick `interval_i = 0`, `rel_data_i = 0`. Back at `1` in layer 0.
5. `next()` → current = `2`. `open()` (depth 1 → depth 2): `parent_start = 0`, `interval_i = 0 + 1 = 1`. `rel_data = layer1.child_data(1) = [4]`. Current = `4`. Emit `[2, 4]`. Done.

## When to prefer this structure

- Large, mostly-static relations where iteration speed and compact layout matter.
- Cache-bound workloads — sequential `data` arrays beat pointer-chased nodes.
- Read-heavy workloads. Avoid for incremental-insert workloads where the layer-shift cost dominates.

## See also

- [`TreeTrie`](./tree-trie.md) — pointer-based alternative.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md) — primary algorithm consumer.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; `ColumnTrie` must pass all 14 patterns under every algorithm (Priorities item 1).
