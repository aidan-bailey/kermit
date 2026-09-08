# `HashTrie`

> **Status:** experimental · **CLI:** `-i hash-trie` · **Implementation:** [`kermit_ds::ds::hash_trie`](../../kermit-ds/src/ds/hash_trie/)

## Representation

`HashTrie` is a hash-based trie: each level is a hash table whose keys are 64-bit hashes of attribute values, and whose values are either child nodes (inner levels) or tuple chains (leaf level).

```rust
HashTrie {
    header: RelationHeader,
    root:   HashTrieNode,
}

enum HashTrieNode {
    Inner(HashTable<HashTrieNode>),     // depths 0..arity-1
    Leaf(HashTable<Vec<Vec<usize>>>),   // depth arity-1
    Singleton(Vec<usize>),              // pruned subtrie (config.singleton_pruning), depths 1..arity
}

struct HashTable<V> {
    log2_capacity: u32,
    len: usize,
    buckets: Vec<Option<Entry<V>>>,     // 2^log2_capacity buckets, open addressing
}

struct Entry<V> { hash: u64, value: V }
```

Bucket index: `hash >> (64 - log2_capacity)` (high `log2_capacity` bits). Collisions are resolved by linear probing within the bucket array. Each occupied bucket stores the full 64-bit hash for disambiguation during probes.

The iterator `HashTrieIter` carries a stack of frames from the root to the current depth. A table frame is `Table { node, idx }` — a bucket within an `Inner` or `Leaf` node; a singleton frame is `Singleton { tuple, depth, hash, exhausted }` and emulates the one-entry table a pruned level would have held, hashing `tuple[depth]` once when the frame is pushed. The deepest frame is the iterator's current position; `HashTrieIter::descent` decides what `open()` descends into (`Descent::Node`, `Descent::Deeper`, or `Descent::Blocked`), so a `Singleton` is never placed in a table frame.

Compared to [`TreeTrie`](./tree-trie.md) and [`ColumnTrie`](./column-trie.md), this structure trades sorted-order navigation for constant-time hash lookup. The cost: hash collisions can produce false-positive intersections at inner levels, which the join algorithm verifies at the leaf via [`verify_and_construct`](../algorithms/hash-triejoin.md).

## Invariants

- **Path depth = arity.** Every root-to-leaf path has length `header.arity()`. Inner nodes at depths `0..arity-1`; leaf nodes at depth `arity-1`. Enforced at construction time by `HashTrie::make_root` and `insert_at`.
- **Hash function consistency.** The hashing convention is the compile-time `HashStrategy` parameter `H`, whose single-argument method `H::hash(key: usize) -> u64` (defined in `kermit_iters::hash_strategy`; `SipHashStrategy` is the default, `FxHashStrategy` the alternative — see [Layout options](#layout-options)) hashes only the attribute value. There is no per-depth parameter: cross-attribute aliasing isn't an issue because attribute positions live in physically distinct hash tables — a value at column 0 and the same value at column 1 are stored in different tables and cannot collide. `HashTrie::insert_at` calls `H::hash(key)` directly; `SingletonHashTrieIter` is handed a precomputed hash by `kermit::db::hash_join`, which uses the same `H`. Both paths must hash with the same `H`, or queries against constants silently break.
- **Multiset semantics.** Duplicate tuples are preserved (added to the same leaf chain) rather than absorbed. This is a deliberate divergence from `TreeTrie`'s set behavior, motivated by the paper's "bag semantics" treatment in §3.2.4. Future enhancement: optional deduplication via a `with_set_semantics` flag.
- **Load factor cap.** Each `HashTable` resizes (doubles) when an insert would push load factor above 0.7. After resize, all entries are rehashed.
- **Leaf chains preserve hash collisions.** Two tuples with identical hash signatures (collisions on every attribute) end up in the same leaf chain. Verification at join time (paper §3.2.3 line 18) distinguishes true matches from false positives. Pinned by the `hash_trie_collisions` tests in [`kermit-ds/tests/hash_trie_tests.rs`](../../kermit-ds/tests/hash_trie_tests.rs), which build the trie under a test-only `hash(k) = k mod 10` strategy so the collisions are real rather than simulated.
- **Pruned iff exactly one tuple.** With `HashTrieConfig::singleton_pruning` on, a child node is `Singleton` iff exactly one tuple lives below it; the shape is insertion-order independent, and a second tuple (including a duplicate or a full hash collision) unprunes the node back into tables. With the flag off, no `Singleton` exists and the structure is identical to pre-pruning builds. Pinned by `check_pruning_invariant` in the [`implementation.rs`](../../kermit-ds/src/ds/hash_trie/implementation.rs) tests.

## Complexity

Let `n` = tuple count, `a` = arity, `b` = max chain length at a leaf bucket.

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | O(a) amortized | O(a) | per-level: one hash + one probe + at most one resize; amortized O(1) per level |
| `from_tuples(n)` | O(n · a) | O(n · a) | loops `insert` over the input; no batch optimization in this first cut |
| `HashTrieIterator::key()` | O(1) | | array access at the deepest stack entry |
| `HashTrieIterator::next()` | O(1) amortized | | scans forward in the current node's bucket array; per-call amortized constant in practice |
| `HashTrieIterator::lookup(h)` | O(1) expected | | linear probe; O(capacity) worst case |
| `HashTrieIterator::size()` | O(1) | | `HashTable::len()` |
| `HashTrieIterator::open()` | O(1) amortized | | pushes a new stack entry, finds first occupied bucket |
| `HashTrieIterator::open()` into a pruned level | O(1) | | pushes a `Singleton` frame; no table probe, one `H::hash` of the next attribute |
| `HashTrieIterator::up()` | O(1) | | pops the stack |
| `HashTrieIterator::leaf_tuples()` | O(1) | | slice of the current bucket's tuple chain |
| `HeapSize::heap_size_bytes()` | O(node count) | | walks the trie recursively summing `HashTable` shell + tuple-chain bytes |

The "amortized O(1)" claims assume good hash distribution (no chronic clustering on linear probes). For pathologically bad inputs (e.g., all keys hashing to the same bucket), `lookup` degrades to O(capacity). The hash function is a Layout choice, not a fixed cost: `fxhash` (`FxHashStrategy`) is implemented and selectable with `--ds-layout-hasher fxhash` — see [Layout options](#layout-options). Other non-cryptographic alternatives (`ahash`, AquaHash) remain unimplemented.

## Worked micro-example

Tuples `{(1, 2), (1, 3), (2, 4)}` build (for some specific hash values — the actual hashes depend on the platform's `DefaultHasher`):

```
HashTrie {
  arity: 2,
  root: Inner(HashTable {
    bucket[i₁]: Some(Entry { hash: h(1), value: Leaf(HashTable {
      bucket[j₁]: Some(Entry { hash: h(2), value: [[1, 2]] }),
      bucket[j₂]: Some(Entry { hash: h(3), value: [[1, 3]] }),
    })}),
    bucket[i₂]: Some(Entry { hash: h(2), value: Leaf(HashTable {
      bucket[j₃]: Some(Entry { hash: h(4), value: [[2, 4]] }),
    })}),
  })
}
```

That is the unpruned shape (`singleton_pruning` off, the default). With pruning on, the child for `1` holds two tuples and stays a `Leaf` table, while the child for `2` holds exactly one and collapses to `Singleton([2, 4])` — so step 6 below pushes a `Singleton` frame instead of a `Table` frame, and its `key()` / `leaf_tuples()` answers are unchanged.

Iteration walk (`hash_trie_iter()`):

1. `open()` → stack: `[Table(root, i₁)]`. `key()` returns `h(1)`.
2. `open()` → stack: `[Table(root, i₁), Table(child_for_1, j₁)]`. `key()` returns `h(2)`. `leaf_tuples()` returns `&[[1, 2]]`.
3. `next()` → stack deepest: `Table(child_for_1, j₂)`. `key()` returns `h(3)`. `leaf_tuples()` returns `&[[1, 3]]`.
4. `next()` → at end at depth 2. `up()` → stack: `[Table(root, i₁)]`.
5. `next()` → stack: `[Table(root, i₂)]`. `key()` returns `h(2)`.
6. `open()` → stack deepest: `Table(child_for_2, j₃)`. `key()` returns `h(4)`. `leaf_tuples()` returns `&[[2, 4]]`. (Pruning on: a `Singleton` frame at depth 1 over `[2, 4]`, with the same `key()` and `leaf_tuples()`.)
7. `up()`, `up()`, `next()` → empty stack. Done.

## When to prefer this structure

- Joins where comparison-based seek (LFTJ's least-upper-bound) is the bottleneck — typically when fan-outs are large and the seek's linear sibling scan dominates.
- Build-once, probe-many workloads where the build cost amortizes across many query evaluations.
- Workloads with high-cardinality join attributes where sorted-trie construction is expensive (sort dominates).

Avoid this structure when ordered iteration of tuples is required. Iteration order at every level depends on hash values relative to the current table capacity — not insertion order, not key order — and the order shifts when a `HashTable` resizes. This affects the order in which result tuples are produced by [`HashTriejoin`](../algorithms/hash-triejoin.md): callers that need a specific order should `ORDER BY` downstream, or prefer [`TreeTrie`](./tree-trie.md) or [`ColumnTrie`](./column-trie.md).

## Optimizations

Per the [optimization standard](../specs/optimization-standard.md), HashTrie's
optimizations are classified into Layout, Config, or BuildMode.

### Layout options

- **Hash function** (`ds_layout_hasher`): selects the hash function used for
  `usize` → `u64` mapping at every level of the trie.
  - **CLI:** `-i hash-trie --ds-layout-hasher <choice>`
  - **Choices:**
    - `sip` (default; `std::collections::hash_map::DefaultHasher`, SipHash-1-3)
    - `fxhash` (`rustc_hash::FxHasher`, fast non-cryptographic)
  - **Type-level:** `HashTrie<H: HashStrategy>` where `H` is one of
    `SipHashStrategy` or `FxHashStrategy` (in `kermit_iters::hash_strategy`).
  - **Bench axis value:** `"sip"` or `"fxhash"`.

### Config flags

- **Singleton pruning** (`ds_config_singleton_pruning`): stores a subtrie
  that holds exactly one tuple as that tuple (paper §3.3.1, Figure 5)
  instead of one hash table per remaining level. Build-time decision;
  the iterator emulates the pruned levels transparently, so
  [`HashTriejoin`](../algorithms/hash-triejoin.md) is unchanged.
  - **CLI:** `-i hash-trie --ds-config singleton-pruning=true` (on `bench ds`
    and `bench run`, the two subcommands that also carry
    `--ds-layout-hasher`).
  - **Default:** `false` (byte-for-byte the pre-pruning structure).
  - **Rust:** `HashTrie::from_tuples_with_config(header, HashTrieConfig { singleton_pruning: true }, tuples)`
    via [`ConfigurableRelation`](../../kermit-ds/src/relation.rs); tests lift
    the value to a type with `Configured<HashTrie<H>, PruningOn>`.
  - **Bench axis value:** `true` / `false`.
  - **Measured effect** (`oxford-uniform-s3`, SipHash, 2026-09-08): space of
    the arity-3 relations −56 % and of the arity-2 relations −22 % (unary
    relations unchanged); insertion ~8–10 % faster; join iteration roughly
    level with pruning off. Supporting the `Singleton` variant costs the
    pruning-*off* configuration about 10 % on join iteration relative to
    the pre-pruning `HashTrie` (about 4 % from the third node variant, the
    rest from the two-variant iterator frame); insertion and space are
    unchanged. Whether pruning helps more under `fxhash` (one cheap hash
    per pruned level instead of a probe) is still untested; use the
    `ds_layout_hasher × ds_config_singleton_pruning` pivot in kermit-lab.

### Deferred follow-ups

- **Skip-levels short-circuit.** The paper's join verifies a singleton
  against the current bindings and skips the remaining levels. That is an
  algorithm-side change (`HashTrieIterator` would expose the singleton and
  `HashTriejoin` would branch on it) and the natural first consumer of the
  reserved `algo_config_*` prefix.
- Lazy child expansion (`ds_config_lazy_expansion`) needs interior
  mutability through `&self` probes; not started.

### Build modes

*None in this release.* See SIGMOD 2020 §3.3.2 for candidate future modes
(`ds_build_mode = "parallel:N"`, `ds_build_mode = "radix:K"`).

## See also

- Sibling docs: [`TreeTrie`](./tree-trie.md), [`ColumnTrie`](./column-trie.md).
- [`HashTriejoin`](../algorithms/hash-triejoin.md) — the only algorithm that consumes this structure.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; `HashTrie` must pass all 11 patterns under `HashTriejoin` (Priorities item 1).
- `hash_trie_test_suite!` and `parquet_test_suite!` ([`kermit-ds/tests/common/macros.rs`](../../kermit-ds/tests/common/macros.rs)) — the layer below the join: `HashTrieIterator` contract (`open`/`next`/`lookup`/`up`/`size`/`leaf_tuples`), construction round-trips via `collect_tuples()`, and Parquet loading. `HashTrie` cannot use `relation_trie_test_suite!` (it is `HashTrieIterable`, not `TrieIterable`), so this hash-family suite mirrors it; each Layout alias (`HashTrieSip`, `HashTrieFx`) runs it, plus the colliding `HashTrieMod10`, and each again as a pruned `Configured<_, PruningOn>` alias so the iterator contract holds on emulated levels too. At the join layer, `define_multiway_join_test_suite_with_config!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) runs the same 11 patterns on the pruned aliases.
