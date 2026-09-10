# `TreeTrie`

> **Status:** stable · **CLI:** `-i tree-trie` · **Implementation:** [`kermit_ds::ds::tree_trie`](../../kermit-ds/src/ds/tree_trie/)

## Representation

`TreeTrie` is a pointer-based trie: each `TrieNode` owns its key (`usize`) and a `Vec<TrieNode>` of children. The trie itself owns the root-level `Vec<TrieNode>`. A tuple `[k_0, k_1, …, k_{n-1}]` is encoded as a root-to-leaf path of depth `n`, where the node at depth `i` holds key `k_i`. Children at every level are kept sorted ascending by key.

```rust
TreeTrie {
    header: RelationHeader,
    children: Vec<TrieNode>,         // root level
}

TrieNode {
    key: usize,
    children: Vec<TrieNode>,
}
```

The iterator `TreeTrieIter` carries a `Vec<(&TrieNode, usize)>` stack from root to the current depth, plus a `sibling_idx` mirroring the deepest stack entry's sibling position. To list the *siblings* of the current node, it reads the parent's `children`: `stack[len - 2]` at depth ≥ 2, or `TreeTrie::children` at depth 1.

Compared to [`ColumnTrie`](./column-trie.md), this layout pays one allocation per node but is structurally local — insertions and splits don't propagate across the layer.

## Invariants

- **Path depth = arity.** Every root-to-leaf path has length `header.arity()`. Enforced at insert by [`TreeTrie::insert`](../../kermit-ds/src/ds/tree_trie/implementation.rs#L161).
- **Sorted children, no duplicate sibling keys.** `children` at every level is sorted ascending by key with no duplicates among siblings. Duplicate tuples are silently absorbed: an insert with an existing key at this level descends into the existing child instead of allocating a new node.
- **Iterator stack invariant (`TreeTrieIter`).** `stack` holds `(node, sibling_index)` pairs from root to the current depth. `sibling_idx` mirrors the sibling index of the deepest stack entry — duplicated so `next` / `seek` can advance it without re-popping.
- **Forward-only `seek`.** `TreeTrieIter::seek(k)` panics if `k < self.key()`. Every `LinearIterator` is positioned at the start of a sorted sequence and may only advance — the leapfrog ring's invariants ensure callers respect this.
- **LFTJ `open`-after-`at_end` discipline.** `TreeTrieIter::open` always descends into `stack.last().children().first()`, even when the deepest sibling has been advanced past the end. LFTJ relies on this — do not "fix" the apparent stale stack top. See the LFTJ gotcha in `CLAUDE.md`.

## Complexity

Let `n` = tuple count, `a` = arity, `b` = average branching factor.

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | O(a · log b + b) | O(a) | binary search locates the slot at each level (a · log b); a diverging key shifts trailing siblings once at the divergence level via `Vec::insert` (b); absorbs duplicates |
| `from_tuples(n)` | O(n · a · log n) | O(n · a) | sort lexicographically, then insert |
| `TrieIterator::key()` | O(1) | | slice index |
| `TrieIterator::next()` | O(1) | | `sibling_idx += 1` |
| `TrieIterator::seek(target)` | O(b) | | linear scan of current siblings |
| `TrieIterator::open()` | O(1) | | push first child |
| `TrieIterator::up()` | O(1) | | pop stack |
| `HeapSize::heap_size_bytes()` | O(node count) | | walks the whole trie summing `Vec` capacities |

`seek` is linear — a binary search would make it `O(log b)`. Acceptable today because sibling fan-outs are usually small; worth noting for high-fan-out workloads.

## Worked micro-example

Tuples `{(1, 2), (1, 3), (2, 4)}` produce:

```
root
├── 1
│   ├── 2
│   └── 3
└── 2
    └── 4
```

Iteration walk (`trie_iter().into_iter()`):

1. `open()` → depth 1, current = `1`.
2. `open()` → depth 2, current = `2`. Emit `[1, 2]`.
3. `next()` → depth 2, current = `3`. Emit `[1, 3]`.
4. `next()` → `at_end()` at depth 2. `up()` → depth 1, current = `1`.
5. `next()` → current = `2`. `open()` → depth 2, current = `4`. Emit `[2, 4]`.
6. `next()` → `at_end()`. `up()` → depth 1. `next()` → `at_end()`. Done.

## When to prefer this structure

- Small or pedagogical relations — the in-memory shape mirrors the conceptual trie.
- Workloads with frequent tuple-by-tuple inserts — `TreeTrie::insert` is local, unlike `ColumnTrie` where an early-layer insert shifts later-layer offsets.
- When debugging algorithm behaviour against the trie shape; pointer chains are easier to inspect than parallel-array offsets.

## See also

- [`ColumnTrie`](./column-trie.md) — column-oriented alternative.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md) — primary algorithm consumer.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; `TreeTrie` must pass all 14 patterns under every algorithm (Priorities item 1).
