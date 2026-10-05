# `HashTrie`

> **Status:** experimental · **CLI:** `-i hash-trie` · **Implementation:** [`kermit_ds::ds::hash_trie`](../../kermit-ds/src/ds/hash_trie/)

## Representation

`HashTrie` is a hash-based trie: each level is a hash table whose keys are 64-bit hashes of attribute values, and whose values are either child nodes (inner levels) or tuple chains (leaf level).

```rust
HashTrie<H: HashStrategy, P: PruningPolicy> {
    header:      RelationHeader,
    root:        HashTrieNode<P>,
    tuple_count: usize,             // multiset size, for Cardinality
    config:      HashTrieConfig,
    _layout:     PhantomData<(H, P)>,
}

enum HashTrieNode<P: PruningPolicy> {
    Inner(HashTable<HashTrieNode<P>>),  // depths 0..arity-1
    Leaf(HashTable<Vec<Vec<usize>>>),   // depth arity-1
    Singleton(P::Payload),              // pruned subtrie, depths 1..arity
}

struct HashTable<V> {
    log2_capacity: u32,
    len: usize,
    buckets: Vec<Option<Entry<V>>>,     // 2^log2_capacity buckets, open addressing
}

struct Entry<V> { hash: u64, value: V }
```

The `Singleton` payload is the second Layout parameter's associated type: `Vec<usize>` under `SingletonPruning`, and the uninhabited `Never` under the default `NoPruning` — so with pruning off the variant cannot be constructed and every `Singleton` arm is dead code the compiler drops. rustc omits uninhabited variants when it computes a layout, so in practice the enum is laid out exactly as it was before pruning existed — an optimisation rustc performs, not a language guarantee, which is why the two size tests `node_does_not_grow_under_the_pruning_policy` (in `implementation.rs`) and `off_frame_is_the_bare_table_pair` (in `hash_trie_iter.rs`) pin it. See [Layout options](#layout-options).

Bucket index: the high `p` bits of `hash × MULTIPLIERS[p]`, where `p = log2_capacity` and `MULTIPLIERS` holds one odd constant per capacity. The paper takes the high bits of the hash itself; multiplying first is this implementation's one departure, and the [bucket-index invariant](#invariants) explains it. Collisions are resolved by linear probing within the bucket array. Each occupied bucket stores the full 64-bit hash for disambiguation during probes.

The iterator `HashTrieIter` carries a stack of frames from the root to the current depth. A table frame is `Table { node, idx }` — a bucket within an `Inner` or `Leaf` node; a singleton frame is `Singleton(P::Frame<'_>)`, whose type is chosen by the pruning Layout parameter: `SingletonFrameOn { tuple, hash, exhausted }` under `SingletonPruning`, emulating the one-entry table a pruned level would have held and hashing the tuple's attribute for that level once when the frame is pushed (the depth itself is the frame's position in the stack, not a stored field); `Never` under `NoPruning`, which makes the variant uninhabited and collapses the frame to the bare `(node, idx)` pair. The deepest frame is the iterator's current position; `HashTrieIter::descent` decides what `open()` descends into (`Descent::Node`, `Descent::Deeper`, or `Descent::Blocked`), so a `Singleton` is never placed in a table frame.

Compared to [`TreeTrie`](./tree-trie.md) and [`ColumnTrie`](./column-trie.md), this structure trades sorted-order navigation for constant-time hash lookup. The cost: hash collisions can produce false-positive intersections at inner levels, which the join algorithm verifies at the leaf via [`verify_and_construct`](../algorithms/hash-triejoin.md).

## Invariants

- **Path depth = arity.** Every root-to-leaf path has length `header.arity()`. Inner nodes at depths `0..arity-1`; leaf nodes at depth `arity-1`. Enforced at construction time by `HashTrie::make_root` and `insert_at`.
- **Hash function consistency.** The hashing convention is the compile-time `HashStrategy` parameter `H`, whose single-argument method `H::hash(key: usize) -> u64` (defined in `kermit_iters::hash_strategy`; `SipHashStrategy` is the default, `FxHashStrategy` the alternative — see [Layout options](#layout-options)) hashes only the attribute value. There is no per-depth parameter: cross-attribute aliasing isn't an issue because attribute positions live in physically distinct hash tables — a value at column 0 and the same value at column 1 are stored in different tables and cannot collide. `HashTrie::insert_at` calls `H::hash(key)` directly; `SingletonHashTrieIter` is handed a precomputed hash by `kermit::db::hash_join`, which uses the same `H`. Both paths must hash with the same `H`, or queries against constants silently break.
- **Multiset semantics.** Duplicate tuples are preserved (added to the same leaf chain) rather than absorbed. This is a deliberate divergence from `TreeTrie`'s set behavior, motivated by the paper's "bag semantics" treatment in §3.2.4. Future enhancement: optional deduplication via a `with_set_semantics` flag.
- **Load factor cap.** Each `HashTable` resizes (doubles) when an insert would push occupancy above the configured cap — `HashTrieConfig::load_factor`, default 0.7 (see [Config flags](#config-flags)). The test is exact integer arithmetic, `(len + 1) * 100 > capacity * percent`. After resize, all entries are rehashed.
- **Bucket index varies with capacity.** A table with `2^p` buckets indexes by the high `p` bits of `hash × MULTIPLIERS[p]` (`HashTable::bucket_index`), and each capacity has its own multiplier: an odd SplitMix64 output, so the multiply loses none of the hash and different capacities' multipliers are unrelated. The paper's `hash >> (64 - p)` is a *prefix* of the index at every larger capacity, so a table's iteration order is also sorted by the index of every smaller capacity. A table rebuilt in that order, such as a `HashTrie` rebuilt from another's `collect_tuples()` or from a projection of it, passes through those smaller capacities as it doubles, and at each one its keys share the lowest buckets. Linear probing turned that into one cluster spanning most of the keys the table held, making the build quadratic in the keys per table (issue #66). A salt fixed per trie depth would not help: the source and the rebuilt table share it. The multiplier covers a rebuild from one table's iteration order or any subset of it. Input that concatenates the iteration orders of two or more large tables of the same capacity, with mostly different keys, still clusters, because their densities add up in the low buckets. No index computed from the hash and the capacity alone can prevent that; only a seed that differs per table instance could. The multiplier costs one table load and one multiply per probe sequence and no space, keeps the structure deterministic, and leaves `heap_size_bytes` unchanged. Pinned by the `rebuilding_*_costs_no_more_than_key_order` tests in [`hash_table.rs`](../../kermit-ds/src/ds/hash_trie/hash_table.rs), which count build probes: the finished table cannot show the difference, because under linear probing a key set's total displacement does not depend on insertion order.
- **Leaf chains preserve hash collisions.** Two tuples with identical hash signatures (collisions on every attribute) end up in the same leaf chain. Verification at join time (paper §3.2.3 line 18) distinguishes true matches from false positives. Pinned by the `hash_trie_collisions` tests in [`kermit-ds/tests/hash_trie_tests.rs`](../../kermit-ds/tests/hash_trie_tests.rs), which build the trie under a test-only `hash(k) = k mod 10` strategy so the collisions are real rather than simulated.
- **Pruned iff exactly one tuple.** Under the `SingletonPruning` Layout, a child node is `Singleton` iff exactly one tuple lives below it; the shape is insertion-order independent, and a second tuple (including a duplicate or a full hash collision) unprunes the node back into tables. Under `NoPruning` no `Singleton` can exist — its payload is uninhabited — and the structure is identical to pre-pruning builds. Pinned by `check_pruning_invariant` in the [`implementation.rs`](../../kermit-ds/src/ds/hash_trie/implementation.rs) tests.

## Complexity

Let `n` = tuple count, `a` = arity, `b` = max chain length at a leaf bucket.

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | O(a) amortized | O(a) | per-level: one hash + one probe + at most one resize; amortized O(1) per level |
| `from_tuples(n)` | O(n · a) | O(n · a) | loops `insert` over the input; no batch optimization in this first cut. Expected cost holds for input in another `HashTrie`'s iteration order, or any subset of it; the bucket-index invariant names the one order it does not cover |
| `HashTrieIterator::key()` | O(1) | | array access at the deepest stack entry |
| `HashTrieIterator::next()` | O(1) amortized | | scans forward in the current node's bucket array; per-call amortized constant in practice |
| `HashTrieIterator::lookup(h)` | O(1) expected | | linear probe; O(capacity) worst case |
| `HashTrieIterator::size()` | O(1) | | `HashTable::len()` |
| `HashTrieIterator::open()` | O(1) amortized | | pushes a new stack entry, finds first occupied bucket |
| `HashTrieIterator::open()` into a pruned level | O(1) | | pushes a `Singleton` frame; no table probe, one `H::hash` of the next attribute |
| `HashTrieIterator::up()` | O(1) | | pops the stack |
| `HashTrieIterator::leaf_tuples()` | O(1) | | slice of the current bucket's tuple chain |
| `HeapSize::heap_size_bytes()` | O(node count) | | walks the trie recursively summing `HashTable` shell + tuple-chain bytes |
| `for_each_tuple(visit)` | O(n) | O(a) stack | depth-first walk lending each stored tuple from its leaf chain or pruned `Singleton`, in `collect_tuples()` order; allocates nothing per tuple, so `bench ds` times it (issue #79). `collect_tuples()` is the same walk, cloning each tuple into a `Vec` |

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

That is the unpruned shape (`--ds-layout-pruning off`, i.e. `HashTrie<H, NoPruning>`, the default). Under `HashTrie<H, SingletonPruning>`, the child for `1` holds two tuples and stays a `Leaf` table, while the child for `2` holds exactly one and collapses to `Singleton([2, 4])` — so step 6 below pushes a `Singleton` frame instead of a `Table` frame, and its `key()` / `leaf_tuples()` answers are unchanged.

Iteration walk (`hash_trie_iter()`):

1. `open()` → stack: `[Table(root, i₁)]`. `key()` returns `h(1)`.
2. `open()` → stack: `[Table(root, i₁), Table(child_for_1, j₁)]`. `key()` returns `h(2)`. `leaf_tuples()` returns `&[[1, 2]]`.
3. `next()` → stack deepest: `Table(child_for_1, j₂)`. `key()` returns `h(3)`. `leaf_tuples()` returns `&[[1, 3]]`.
4. `next()` → at end at depth 2. `up()` → stack: `[Table(root, i₁)]`.
5. `next()` → stack: `[Table(root, i₂)]`. `key()` returns `h(2)`.
6. `open()` → stack deepest: `Table(child_for_2, j₃)`. `key()` returns `h(4)`. `leaf_tuples()` returns `&[[2, 4]]`. (Under `SingletonPruning`: a `Singleton` frame at depth 1 over `[2, 4]`, with the same `key()` and `leaf_tuples()`.)
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

- **Singleton pruning** (`ds_layout_pruning`): stores a subtrie that holds
  exactly one tuple as that tuple (paper §3.3.1, Figure 5) instead of one
  hash table per remaining level. It is a *shape* — fixed at construction,
  it changes which node variants exist — so it is a Layout, not a Config
  (see the classification rule in the
  [optimization standard](../specs/optimization-standard.md#how-to-classify)).
  The decision is taken at build time; the iterator emulates the pruned
  levels transparently, so [`HashTriejoin`](../algorithms/hash-triejoin.md)
  is unchanged.
  - **CLI:** `-i hash-trie --ds-layout-pruning <off|on>`
  - **Choices:**
    - `off` (default; `NoPruning`, the pre-pruning structure)
    - `on` (`SingletonPruning`, paper Figure 5)
  - **Type-level:** `HashTrie<H, P: PruningPolicy>` where `P` is
    `NoPruning` or `SingletonPruning` (in
    [`pruning.rs`](../../kermit-ds/src/ds/hash_trie/pruning.rs)).
    `P::Payload` is what `HashTrieNode::Singleton` holds: `Vec<usize>` when
    on, the uninhabited `Never` when off. The `off` instantiation compiles
    to the pre-pruning code (pinned by
    `node_does_not_grow_under_the_pruning_policy` and
    `off_frame_is_the_bare_table_pair`); measured at parity with the
    pre-pruning baseline on `oxford-uniform-s3` (byte-identical space,
    iteration inside the TreeTrie control's spread); see the design spec's
    Amendment 1 § D acceptance record. Under `fxhash` the pruning-on join
    iteration gain on `triangle` grows from −3 % to −11 % (one cheap hash
    replaces a probe per pruned level).
  - **Test aliases:** `HashTrieSipPruned` and `HashTrieFxPruned` at the
    join layer; all three pruned aliases — those two plus the colliding
    `HashTrieMod10Pruned` — at the DS layer.
  - **Bench axis value:** `"off"` or `"on"`.
  - **Measured effect** (`oxford-uniform-s3`, SipHash, 2026-09-08, measured
    while pruning was still a Config): space of the arity-3 relations
    −56 % and of the arity-2 relations −22 % (unary relations unchanged, as
    they have no level to prune); insertion ≈ 9 % faster. Join iteration
    lands between the two pruning-off numbers: triangle ≈ 1.92 ms against
    2.24 ms with pruning off on the same branch (the pre-pruning baseline
    was ≈ 1.86 ms), and binary-join level with pruning off. Whether pruning
    helps more under
    `fxhash` (one cheap hash per pruned level instead of a probe) is still
    untested; use the `ds_layout_hasher × ds_layout_pruning` pivot in
    kermit-lab.

### Config flags

- **Load factor cap** (`ds_config_load_factor`): the occupancy a level's
  hash table is allowed to reach before it doubles. This is a *value* read
  on a path the code already takes — the resize comparison in
  `HashTable::entry_or_insert_with`, `(len + 1) * 100 > capacity * percent`
  — so replacing the former compile-time constant with it adds no branch
  and non-users pay nothing. `HashTrie` holds the config and passes the cap
  down through `insert_at` into the table call; nothing is stored per
  table, so space is unchanged.
  - **CLI:** `-i hash-trie --ds-config load-factor=0.5` (on `bench ds` and
    `bench run`, the two subcommands that also carry the `--ds-layout-*`
    flags). The value is a decimal in the open interval (0, 1) with at most
    two decimal places; anything else is a usage error naming the range.
  - **Default:** `0.7` (the historical constant).
  - **Rust:** `HashTrieConfig { load_factor: LoadFactor::percent(50)? }`
    reaches the trie through
    [`ConfigurableRelation`](../../kermit-ds/src/relation.rs)'s
    `from_tuples_with_config`; `LoadFactor` is an exact percentage in
    `1..=99`, so the resize test stays integer arithmetic. Tests lift the
    value to a type with `Configured<HashTrie<H, P>, HalfFull>`.
  - **Bench axis value:** the JSON number `0.7` / `0.5` / `0.9`.
  - **Expected effect:** space and iteration move in opposite directions
    along this axis — a higher cap packs the tables denser (less memory,
    longer probe chains), a lower cap does the reverse. FxHash's poorer
    distribution on structured keys should make it more sensitive to a high
    cap than SipHash, which makes `ds_layout_hasher × ds_config_load_factor`
    the first 2 × 2 where the two metrics are expected to disagree.
    Unmeasured as of this writing.

### Deferred follow-ups

- **Skip-levels short-circuit — shelved, not in the paper
  ([#90](https://github.com/aidan-bailey/kermit/issues/90)).** A join that
  checks a singleton against the current bindings and skips its remaining
  levels is not described by the VLDB 2020 paper or by the TUM-I2082
  technical report it defers to. Both present singleton pruning as a storage
  layout only — a tagged child pointer straight to the tuple (§3.3.1) — and
  the probe phase is Algorithm 3, unrolled (§3.3.3). Pruning therefore stays
  transparent to `HashTriejoin`: each emulated level is a one-entry table,
  which Algorithm 3's `argmin size` already picks as `I_scan`. Were it
  revived as a kermit-specific extension, it would be an algorithm *Layout*,
  not a Config: `JoinAlgo::join_for_each` takes no `self` to hold a runtime
  value, and checking for a pruned child before each `open` is a branch that
  runs without the extension would pay for.
- **Lazy child expansion** is a *Layout* candidate, not a Config one: an
  unexpanded node is a node state and needs a cell in `HashTrieNode` that
  eager tries would carry for nothing. It also needs interior mutability
  through `&self` probes. Not started.

### Build modes

*None in this release.* See SIGMOD 2020 §3.3.2 for candidate future modes
(`ds_build_mode = "parallel:N"`, `ds_build_mode = "radix:K"`).

## See also

- Sibling docs: [`TreeTrie`](./tree-trie.md), [`ColumnTrie`](./column-trie.md).
- [`HashTriejoin`](../algorithms/hash-triejoin.md) — the only algorithm that consumes this structure.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; every Layout combination of `HashTrie` (`HashTrieSip`, `HashTrieFx`, `HashTrieSipPruned`, `HashTrieFxPruned`) must pass all 16 patterns under `HashTriejoin`, with both optimisers (Priorities item 1).
- `hash_trie_test_suite!` and `parquet_test_suite!` ([`kermit-ds/tests/common/macros.rs`](../../kermit-ds/tests/common/macros.rs)) — the layer below the join: `HashTrieIterator` contract (`open`/`next`/`lookup`/`up`/`size`/`leaf_tuples`), construction round-trips via `collect_tuples()`, and Parquet loading. `HashTrie` cannot use `relation_trie_test_suite!` (it is `HashTrieIterable`, not `TrieIterable`), so this hash-family suite mirrors it; each Layout alias runs it — `HashTrieSip`, `HashTrieFx` and the colliding `HashTrieMod10`, and each again with pruning on (`HashTrieSipPruned`, `HashTrieFxPruned`, `HashTrieMod10Pruned`) so the iterator contract holds on emulated levels too — plus `HashTrieSipDense = Configured<HashTrieSip, NinetyPercent>` and `HashTrieMod10Dense` for the Config axis. At the join layer, `define_multiway_join_test_suite_with_config!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) runs the same 16 patterns under the `HalfFull` load-factor provider.
