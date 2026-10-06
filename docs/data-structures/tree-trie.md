# `TreeTrie`

> **Status:** stable · **CLI:** `-i tree-trie` · **Implementation:** [`kermit_ds::ds::tree_trie`](../../kermit-ds/src/ds/tree_trie/)

## Representation

`TreeTrie` is a pointer-based trie: each `TrieNode` owns its key (`usize`) and a `Vec<TrieNode>` of children. The trie itself owns the root-level `Vec<TrieNode>`. A tuple `[k_0, k_1, …, k_{n-1}]` is encoded as a root-to-leaf path of depth `n`, where the node at depth `i` holds key `k_i`. Children at every level are kept sorted ascending by key.

```rust
TreeTrie<S: SeekStrategy = GallopingSeek> {
    header: RelationHeader,
    children: Vec<TrieNode>,         // root level
    tuple_count: usize,              // distinct tuples; backs `Cardinality`
    _seek: PhantomData<S>,           // seek strategy, zero-sized
}

TrieNode {
    key: usize,
    children: Vec<TrieNode>,
}
```

The iterator `TreeTrieIter` carries a `Vec<(&TrieNode, usize)>` stack from root to the current depth, plus a `sibling_idx` cursor within the current sibling list. To list the *siblings* of the current node, it reads the parent's `children`: `stack[len - 2]` at depth ≥ 2, or `TreeTrie::children` at depth 1.

Compared to [`ColumnTrie`](./column-trie.md), this layout pays one allocation per node but is structurally local — insertions and splits don't propagate across the layer.

## Invariants

- **Path depth = arity.** Every root-to-leaf path has length `header.arity()`. Enforced at insert by [`TreeTrie::insert`](../../kermit-ds/src/ds/tree_trie/implementation.rs#L161).
- **Sorted children, no duplicate sibling keys.** `children` at every level is sorted ascending by key with no duplicates among siblings. Duplicate tuples are silently absorbed: an insert with an existing key at this level descends into the existing child instead of allocating a new node.
- **Iterator stack invariant (`TreeTrieIter`).** `stack` holds `(node, sibling_index)` pairs from root to the current depth. `sibling_idx` is the authoritative cursor. It equals the deepest entry's index except past the end: when `next` or a failed `seek` runs off the last sibling, `sibling_idx` becomes `siblings.len()` and the entry keeps the last node the iterator was positioned on.
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
| `TrieIterator::seek(target)` | `S`-dependent: linear O(d), binary O(log r), galloping O(log d) | | `S::partition_point` over the `r` remaining siblings; `d` is the distance moved. See [seek strategies](seek-strategies.md) |
| `TrieIterator::open()` | O(1) | | push first child |
| `TrieIterator::up()` | O(1) | | pop stack |
| `HeapSize::heap_size_bytes()` | O(node count) | | walks the whole trie summing `Vec` capacities |

`seek` asks its seek strategy `S` how many of the remaining siblings lie below the target, and moves that far. The strategy is a Layout shared with `ColumnTrie`, so the two sorted tries differ only in layout under any one strategy. The default is `galloping`, the fastest of the three on #80's probe set; `binary`, the `partition_point` search both tries used before the parameter existed, was the default until 2026-10-05. See [seek strategies](seek-strategies.md) for the three strategies, their probe bounds and the LFTJ bound they relate to.

Until issue #67, `seek` was a linear scan. On high-fan-out WatDiv queries it made `TreeTrie` 10–197x slower than `ColumnTrie`, and `TreeTrie` numbers from before that fix are not comparable with later ones. `--ds-layout-seek linear` runs the same algorithm in today's code, not the pre-#67 code. `seek_cost_matches_the_strategy` in `trie_seek_tests!` ([`kermit-ds/tests/common/macros.rs`](../../kermit-ds/tests/common/macros.rs)) pins each strategy's complexity through the real iterator.

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

## Optimizations

| Dimension | Category | Axis | Flag | Default | Test aliases |
|---|---|---|---|---|---|
| Seek strategy | Layout (`S: SeekStrategy`) | `ds_layout_seek` | `--ds-layout-seek linear\|binary\|galloping` | `galloping` | `TreeTrieLinear`, `TreeTrieBinary`, `TreeTrieGalloping` |
| Build | BuildMode (`TreeTrieBuildMode`) | `ds_build_mode` | `--ds-build tree-trie=serial\|parallel:N` | `serial` | `TreeTrieParallel2` (`BuiltWith<TreeTrie, Parallel2>`) |

The strategy changes only how `seek` searches; it changes no stored data, no build and no `heap_size_bytes`. Details: [seek strategies](seek-strategies.md).

The build mode changes only how long the build takes: `parallel:N` builds the identical trie, down to every `Vec`'s capacity, on N threads. Details: [parallel builds](parallel-build.md); measured speedups: [its scaling result](parallel-build.md#scaling-result-treetrie-2026-10-05).

## See also

- [`ColumnTrie`](./column-trie.md) — column-oriented alternative.
- [Seek strategies](./seek-strategies.md) — the `S` Layout shared with `ColumnTrie`.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md) — primary algorithm consumer.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; `TreeTrie` must pass all 16 patterns under every algorithm (Priorities item 1).
