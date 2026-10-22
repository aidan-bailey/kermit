# `HashTrie`

> **Status:** experimental · **CLI:** `-i hash-trie` · **Implementation:** [`kermit_ds::ds::hash_trie`](../../kermit-ds/src/ds/hash_trie/)

## Representation

`HashTrie` is a hash-based trie: each level is a hash table whose keys are 64-bit hashes of attribute values, and whose values are either child nodes (inner levels) or tuple chains (leaf level). A chain is a list of its tuples' row ids threaded through the trie's links, one 4-byte link per row beside the buffer; the tuples themselves live once, in the trie's buffer.

```rust
HashTrie<H: HashStrategy, P: PruningPolicy, E: ExpansionPolicy> {
    header:      RelationHeader,
    tuples:      Tuples,            // every stored tuple: arrival or partition order (below); len() = multiset size
    links:       ChainLinks,        // one link per row of `tuples`: the next row in its list, or NIL
    root:        HashTrieNode<P, E>,
    config:      HashTrieConfig,
    _layout:     PhantomData<(H, P, E)>,
}

enum HashTrieNode<P: PruningPolicy, E: ExpansionPolicy> {
    Inner(HashTable<HashTrieNode<P, E>>),     // depths 0..arity-1
    Leaf(HashTable<Chain>),                   // depth arity-1: the chains
    Singleton(P::Payload),                    // pruned subtrie, depths 1..arity
    Unexpanded(E::Pending<HashTrieNode<P, E>>), // lazy child, depths 1..arity
}

struct ChainLinks(Vec<AtomicU32>);           // kermit-iters; Relaxed loads and stores

struct Chain {                                // a list threaded through `links`, in input order
    head: RowId,
    tail: RowId,
    len:  NonZeroU32,
}

struct LazyChild<N> {                         // E::Pending<N> under LazyExpansion (boxed)
    pending: Cell<Option<Chain>>,             // the rows below the bucket, until a probe takes them
    built:   OnceCell<N>,                     // the table, once a probe reached it
}

struct HashTable<V> {
    log2_capacity: u32,
    len: usize,
    buckets: Vec<Option<Entry<V>>>,     // 2^log2_capacity buckets, open addressing
}

struct Entry<V> { hash: u64, value: V }
```

The `Singleton` payload is the second Layout parameter's associated type: the `RowId` of its one tuple under `SingletonPruning`, and the uninhabited `Never` under the default `NoPruning` — so with pruning off the variant cannot be constructed and every `Singleton` arm is dead code the compiler drops. rustc omits uninhabited variants when it computes a layout, so in practice the enum is laid out exactly as it was before pruning existed — an optimisation rustc performs, not a language guarantee, which is why the two size tests `node_does_not_grow_under_the_pruning_policy` (in `implementation.rs`) and `off_frame_is_the_bare_table_pair` (in `hash_trie_iter.rs`) pin it. See [Layout options](#layout-options).

The `Unexpanded` payload is the third Layout parameter's associated type, by the same device: `Box<LazyChild<N>>` under `LazyExpansion` and `Never` under the default `EagerExpansion`, pinned by `node_does_not_grow_under_the_expansion_policy`. Under lazy expansion only the root is built at construction; every child below it keeps its tuples as a pending `Chain` until `HashTrieIter::open` first enters it, and `HashTrie::resolve` then takes the chain (`Cell::take`) and re-threads its rows into a table built by Algorithm 2's `build` (`bulk.rs`), one level deep (its own children start unexpanded). The re-threading writes links through `&self`, which is why the links are atomics (spec Decision 3). `for_each_tuple` reads `built` if present and walks `pending` otherwise; `heap_size_bytes` counts the `Box` and `built`, since a pending chain owns no heap; `collect_tuples` and `scan_tuples` read the buffer; none of them expands anything.

**The tuple buffer** (#111, #101). The trie owns its relation's tuples as one row-major `Tuples` batch: the rows its build copied out of a `TupleView`, then every row `insert` appends. Every build makes exactly one copy of the view's matching rows, at exact size, and keeps it as the buffer (per-query index builds, 2026-10-22; before them `bulk` and `incremental` kept the caller's batch without a copy). `bulk` and `incremental` copy in arrival order (`view.materialize()`). `radix:K`, `parallel:N` and `presized:N` copy into partition order on the first attribute's hash (`morsel::partition_rows`, the paper's materialisation step, §3.3.2): the counting pass tests `view.matches(row)` and the scatter calls `view.write(row, out)`, so each partition's rows are one contiguous range of the buffer, in input order, and the buffer holds only the rows the filter keeps. The build only reads the view's source, which is the catalog's flat area and outlives the trie. Below the tables every tuple is named by its `RowId` (`u32`, an index into the buffer): a pruned `Singleton` is one `RowId`, and every list (a leaf chain, a lazy pending list, and each of Algorithm 2's per-bucket lists while a build runs) is a `Chain` threaded through `links`, in input order (#101 B2). `HashTrieIterator::leaf_tuples` lends a chain as a `LeafRows` view over the buffer and the links. Two builds that copy in arrival order own equal buffers, so the `bulk`/`incremental` identity tests compare ids and `links` arrays, after checking the buffers are equal; the partitioning modes are compared by contents, each chain walked through its own trie's links and read through its own buffer (`identity::assert_same_contents`). Reading back: `for_each_tuple` walks the trie depth-first, each chain through `links` (`bench ds` times it, #79); `TupleScan::scan_tuples` (statistics, which need only the multiset) scans the buffer in its row order; `collect_tuples` copies the buffer.

**The chains** (#101 B2). `links: ChainLinks` (`kermit-iters`) holds one link per row: the id of the next row in that row's list, or `ChainLinks::NIL` (`u32::MAX`, never a row, since a batch holds at most `u32::MAX` rows). A list is a `Chain { head, tail, len }` stored in its bucket ([`chain.rs`](../../kermit-ds/src/ds/hash_trie/chain.rs)). `Chain::start` writes `NIL` into its row's link, and `Chain::push` appends a row by writing two links, the old tail's and the row's own `NIL`. Algorithm 2 moves each row from its parent's list to exactly one child list by rewriting its link, so a row's link is rewritten once per level and ends as its leaf-chain successor. A chain's walk (`ChainWalk`) reads a row's link before it yields the row, so the build may re-thread the rows it is walking, and it stops after `len` rows. `len` serves line 3's `|L|`, the pruning test and `LeafRows::len`; as a `NonZeroU32` it gives `Option<Entry<Chain>>` a niche, so a leaf bucket is 24 B (8 for the hash, 12 for the chain, padded), where it was 32 B plus a heap list. A build therefore allocates per table, never per list: the tables, the buffer (the one copy of the view's rows), one `links` array, and the scratch structures sized up front.

| Element (VLDB 2020 §3.2.2, §3.3.1–3.3.2, Fig. 3) | kermit since #101 B2 (2026-10-14) |
| --- | --- |
| Tuples in one contiguous buffer, fixed-length layout | ✓ the relation's buffer, row-major (#111) |
| Leaves, singletons and lazy children refer to tuple memory | ✓ by row id (4 B, kermit's; the paper's pointers are 8 B) |
| Tuples partitioned by the first key's hash as they are materialised | ✓ in `radix:K`, `parallel:N` and `presized:N` (`morsel::partition_rows`); ✗ arrival order in `bulk` (the default) and `incremental` |
| Partitioning morsel-driven, on N threads | ✓ the copy itself, in `parallel:N` and `presized:N` |
| Two-pass radix partitioning (Balkesen et al.) | ✗ one round of partitions (kermit's: #101 found no fan-out cost at this scale, `radix:16` within 1.5 % of `radix:12` on grouped input) |
| A leaf is a list threaded through a chain pointer per tuple | ✓ a link per tuple (#101 B2), with two departures, both kermit's: a 4-byte id in an array beside the tuple rather than an 8-byte pointer inside it (the design's Decision 1); lists in input order, appended rather than pushed (Decision 2). Before B2, a `Vec<RowId>` per chain |
| Algorithm 2 moves each tuple to exactly one child list | ✓ by rewriting its link; before B2 it copied the tuple's id into a new `Vec` |
| No allocation per list | ✓ since #101 B2 |
| A bucket is 16 B (8-byte hash, 8-byte child slot) | ✗ a leaf bucket is 24 B: the hash and a `Chain` (head, tail and `len`) behind an `Option` (kermit's; 32 B plus a heap list before B2). An inner bucket holds its child's table header inline (#106) |
| Lazy expansion re-threads a child's tuples on its first probe | ✓ through relaxed atomics (Decision 3); before B2 it moved a `Vec` |

The paper's buffer is a copy its build makes. Every mode here makes one from the view, and the `build` metric times it. `bulk` and `incremental` copy in arrival order, one pass over the matching rows, and pay nothing for partitioning. The partitioning modes make the paper's partitioned copy and pay for it:
- a histogram pass over the source's rows and a scatter of the matching ones;
- while the copy is made, the view's source and the copy both held (the source is the catalog's and is never freed by the build);
- 4 bytes per row of `origin`, each copied row's input position, held until the scratch roots are built (`radix:K` and `parallel:N` free it before the merge, `presized:N` right after its tail), so the root merge and `presized:N`'s tail keep `bulk`'s order;
- 2 bytes per row of partition numbers (`u16`), held between the histogram and the scatter, so each row is hashed once.

The chains cost 4 bytes per row for the trie's life (`links`, counted by `heap_size_bytes`), and one store per push beyond the paper's: the `NIL` written into the pushed row's link, which keeps `links` a function of the lists (kermit's). In return no build allocates per list. A chain step reads two places, the link and then the row, where the paper's in-row pointer reads one cache line; the links are dense, and in the partitioning modes a chain's rows are partition-local (#101 B1). The links are `AtomicU32`, read and written `Relaxed`, so that a lazy trie can re-thread a child's rows inside `HashTrieIter::open`, which holds only `&self`. On x86-64 and AArch64 a relaxed load or store compiles to a plain one; eager tries use the same atomics, which keeps them `Sync`, and what that costs them is measured once (the design's § Measurement).

**Measured effect** (2026-10-18, [Chain links, measured](#chain-links-measured-101-b2)). On the 33 JOB templates the chains cut HashTrie's peak memory by about 15 %: before B2 a run peaked at 1.13–1.26× today's (geomean 1.18), with the same answers. In the replicated B1 + B2 run, which was stopped part-way, eager builds take 0.67–0.90× as long as before and the triangle join runs at the same speed (1.01–1.02×). The one regression is a lazy trie's walk of its unexpanded chains on ungrouped input, up to 11× slower: a `Vec<RowId>`'s loads were independent, and a chain's are dependent, one link per row, scattered over the buffer.

Design: [`docs/specs/2026-10-08-hash-trie-partitioned-materialisation-design.md`](../specs/2026-10-08-hash-trie-partitioned-materialisation-design.md) (#101 B1) and [`docs/specs/2026-10-14-hash-trie-chain-links-design.md`](../specs/2026-10-14-hash-trie-chain-links-design.md) (#101 B2).

Bucket index: the high `p` bits of `hash × MULTIPLIERS[p]`, where `p = log2_capacity` and `MULTIPLIERS` holds one odd constant per capacity. The paper takes the high bits of the hash itself; multiplying first is this implementation's one departure, and the [bucket-index invariant](#invariants) explains it. Collisions are resolved by linear probing within the bucket array. Each occupied bucket stores the full 64-bit hash for disambiguation during probes.

The iterator `HashTrieIter` carries a stack of frames from the root to the current depth. A table frame is `Table { node, idx }` — a bucket within an `Inner` or `Leaf` node; a singleton frame is `Singleton(P::Frame<'_>)`, whose type is chosen by the pruning Layout parameter: `SingletonFrameOn { row, hash, exhausted }` under `SingletonPruning`, emulating the one-entry table a pruned level would have held and hashing the attribute of its row (read from the buffer through the row id) for that level once when the frame is pushed (the depth itself is the frame's position in the stack, not a stored field); `Never` under `NoPruning`, which makes the variant uninhabited and collapses the frame to the bare `(node, idx)` pair. The deepest frame is the iterator's current position; `HashTrieIter::descent` decides what `open()` descends into (`Descent::Node`, `Descent::Deeper`, or `Descent::Blocked`), so a `Singleton` is never placed in a table frame.

Compared to [`TreeTrie`](./tree-trie.md) and [`ColumnTrie`](./column-trie.md), this structure trades sorted-order navigation for constant-time hash lookup. The cost: hash collisions can produce false-positive intersections at inner levels, which the join algorithm verifies at the leaf via [`verify_and_construct`](../algorithms/hash-triejoin.md).

### Construction

A trie built from a view of known tuples (`from_view`; `from_tuples` is the convenience over the identity view) first copies the view's matching rows into its buffer, then is built by the paper's
Algorithm 2 (VLDB 2020, §3.2.2; [`bulk.rs`](../../kermit-ds/src/ds/hash_trie/bulk.rs)):
a table is allocated for a list of tuples, every tuple is pushed onto the
list in its bucket, and each bucket's list is then built into the bucket's
child, recursively. This is the `bulk` build mode, the default.

| Algorithm 2 | kermit |
|---|---|
| before Algorithm 2, the input "materialized contiguously in an in-memory buffer" and "partitioned based on the hash values of the first join key attribute" (§3.3.2) | every build copies the view's matching rows once and keeps the copy: `bulk` and `incremental` in arrival order, applying the filter as they copy (kermit's); `radix:K`, `parallel:N` and `presized:N` into partition order (`morsel::partition_rows`, #101) ([Build modes](#build-modes)) |
| line 3, allocate `M` | `HashTable::with_log2_capacity`, at the capacity `root_log2_capacity` (the root) or `child_log2_capacity` (a child) gives |
| line 3, the size of `M`, `2^⌈log2(1.25·\|L\|)⌉` | by default 4 buckets that double at the load factor as keys arrive, so a table is not allocated once (kermit's); `root-capacity=tuples,child-capacity=tuples` at `load-factor=0.8` is the paper's sizing, every table allocated once ([Config flags](#config-flags)) |
| lines 4–7, push each tuple onto its bucket's list | `HashTrie::group`, through `chain::push_row`: a fresh bucket starts a chain at the row (one link write), any other appends the row at its chain's tail (two). Line 7's push, read literally, prepends; kermit's lists append, so they keep input order |
| lines 8–12, build each bucket's child from its list | `HashTrie::build_nested`, through `HashTable::map` and `HashTrie::child`. Line 10's extract moves nothing: the child is grouped from the chain's walk, which reads each row's link before the row is re-threaded |
| line 15, return the list | the last attribute's table of lists is the `Leaf`, its lists the chains |
| the lists themselves | a `Chain { head, tail, len }` per bucket, threaded through `links`, one 4-byte link per row of the trie's tuple buffer (#101 B2): the paper's chain pointer per tuple (§3.3.2), as a 4-byte id in an array beside the buffer rather than an 8-byte pointer inside each tuple (kermit's, after #111's 4-byte ids). Moving a row to a child list rewrites its link, so no list allocates |

`child` decides what a bucket's list becomes: a `Singleton` under pruning
(§3.3.1), an `Unexpanded` child under lazy expansion (§3.3.1), and
otherwise the next table, line 11.

**Against the per-tuple build.** `insert`, and the `incremental` build
mode, place one tuple at a time by `insert_at` instead. Under every config
both accept (`incremental` accepts only `child-capacity=grow`), the two
builds give the identical trie, `links` included. A table's layout depends
only on the order its new keys arrive, and every list keeps input order,
because a push appends. Every push writes `NIL` into the pushed row's link,
so `links` is a function of the lists alone (#101 B2), and a chain owns no
heap, so no capacity can differ either.

## Invariants

- **Path depth = arity.** Every root-to-leaf path has length `header.arity()`. Inner nodes at depths `0..arity-1`; leaf nodes at depth `arity-1`. Enforced at construction time by `make_root_sized` and the builds (`build_nested`, `insert_at`).
- **Hash function consistency.** The hashing convention is the compile-time `HashStrategy` parameter `H`, whose single-argument method `H::hash(key: usize) -> u64` (defined in `kermit_iters::hash_strategy`; `SipHashStrategy` is the default, `FxHashStrategy` the alternative — see [Layout options](#layout-options)) hashes only the attribute value. There is no per-depth parameter: cross-attribute aliasing isn't an issue because attribute positions live in physically distinct hash tables — a value at column 0 and the same value at column 1 are stored in different tables and cannot collide. `HashTrie::insert_at` calls `H::hash(key)` directly, and so does every build. A constant is a filter applied while the rows are copied, so no hash is made for it outside the trie. Before per-query index builds (2026-10-22), a one-value iterator was handed a precomputed hash by `kermit::db::hash_join`, and both paths had to hash with the same `H` or queries against constants silently broke.
- **Multiset semantics.** Duplicate tuples are preserved (added to the same leaf chain) rather than absorbed. This is a deliberate divergence from `TreeTrie`'s set behavior, motivated by the paper's "bag semantics" treatment in §3.2.4. Future enhancement: optional deduplication via a `with_set_semantics` flag. `Relation::KEEPS_DUPLICATES` is `true` for this structure alone: an atom with no kept column (only constants) builds no index, and its row count multiplies the join's results here, where the sorted tries clamp it to 1.
- **Load factor cap.** Each `HashTable` resizes (doubles) when an insert would push occupancy above the configured cap — `HashTrieConfig::load_factor`, default 0.7 (see [Config flags](#config-flags)). The test is exact integer arithmetic, `(len + 1) * 100 > capacity * percent`. After resize, all entries are rehashed. Every table starts at 4 buckets except in a build from a known set of tuples, where `--ds-config root-capacity=tuples` sizes the root once for the tuple count, and `child-capacity=tuples` sizes every child once for its list (see [Config flags](#config-flags)).
- **Bucket index varies with capacity.** A table with `2^p` buckets indexes by the high `p` bits of `hash × MULTIPLIERS[p]` (`HashTable::bucket_index`), and each capacity has its own multiplier: an odd SplitMix64 output, so the multiply loses none of the hash and different capacities' multipliers are unrelated. The paper's `hash >> (64 - p)` is a *prefix* of the index at every larger capacity, so a table's iteration order is also sorted by the index of every smaller capacity. A table rebuilt in that order, such as a `HashTrie` rebuilt from another's `for_each_tuple` walk (or, before #111, from its `collect_tuples()` or a projection of it, which walked the trie too; both now read its buffer, in its row order), passes through those smaller capacities as it doubles, and at each one its keys share the lowest buckets. Linear probing turned that into one cluster spanning most of the keys the table held, making the build quadratic in the keys per table (issue #66). A salt fixed per trie depth would not help: the source and the rebuilt table share it. The multiplier covers a rebuild from one table's iteration order or any subset of it. Input that concatenates the iteration orders of two or more large tables of the same capacity, with mostly different keys, still clusters, because their densities add up in the low buckets. No index computed from the hash and the capacity alone can prevent that; only a seed that differs per table instance could. The multiplier costs one table load and one multiply per probe sequence and no space, keeps the structure deterministic, and leaves `heap_size_bytes` unchanged. Pinned by the `rebuilding_*_costs_no_more_than_key_order` tests in [`hash_table.rs`](../../kermit-ds/src/ds/hash_trie/hash_table.rs), which count build probes: the finished table cannot show the difference, because under linear probing a key set's total displacement does not depend on insertion order.
- **Leaf chains preserve hash collisions.** Two tuples with identical hash signatures (collisions on every attribute) end up in the same leaf chain. Verification at join time (paper §3.2.3 line 18) distinguishes true matches from false positives. Pinned by the `hash_trie_collisions` tests in [`kermit-ds/tests/hash_trie_tests.rs`](../../kermit-ds/tests/hash_trie_tests.rs), which build the trie under a test-only `hash(k) = k mod 10` strategy so the collisions are real rather than simulated.
- **Links are a function of the lists** (#101 B2). `links.len() == tuples.len()`, and every row's link is its successor in the one list that holds it, or `NIL`: a `Singleton`'s row and every list's tail hold `NIL`, because every push writes `NIL` into the pushed row's link. Two builds that make the same lists therefore make the same `links`, so `bulk` and `incremental` are array-identical, links included. `insert` pushes one `NIL` link per row it appends, and the per-row build does the same before each `insert_at`; zeroed links would leave a pruned `Singleton`'s row at 0 where `bulk` writes `NIL`. Pinned by `bulk_and_incremental_thread_the_same_links` in [`bulk.rs`](../../kermit-ds/src/ds/hash_trie/bulk.rs), and by `identity::assert_same_trie`, which compares the two `links` arrays whole.
- **Lazy buckets hold no tables.** Under the `LazyExpansion` Layout, every `Inner` bucket holds a `Singleton` (pruning on, exactly one tuple below it) or an `Unexpanded` child, never a table; a table appears only inside an `Unexpanded` child a probe has built. An expanded child's table is the eager table at that position, bucket for bucket, for a trie no `insert` has changed since its build, and always under `child-capacity=grow`: it is built by Algorithm 2 from its pending list, which keeps insertion order, under the same load factor and child capacity as the eager build. Pinned by the `lazy_expansion` trace tests in [`kermit-ds/tests/hash_trie_tests.rs`](../../kermit-ds/tests/hash_trie_tests.rs), which require identical probe traces from an eager and a lazy trie.
- **Pruned iff exactly one tuple.** Under the `SingletonPruning` Layout, a child node is `Singleton` iff exactly one tuple lives below it; the shape is insertion-order independent, and a second tuple (including a duplicate or a full hash collision) unprunes the node back into tables. Under `NoPruning` no `Singleton` can exist — its payload is uninhabited — and the structure is identical to pre-pruning builds. Pinned by `check_pruning_invariant` in the [`implementation.rs`](../../kermit-ds/src/ds/hash_trie/implementation.rs) tests.

## Complexity

Let `n` = tuple count, `a` = arity, `b` = max chain length at a leaf bucket.

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | O(a) amortized | O(a) | per-level: one hash + one probe + at most one resize; amortized O(1) per level. The row is appended to the buffer with a `NIL` link, and the leaf step starts a chain at it or appends it to one: one or two link writes, no allocation (#101 B2) |
| `from_view(view)` (`from_tuples(n)` over the identity view) | O(n · a) | O(n · a) | the default `bulk` build: one `materialize()` copy of the matching rows (a filtered view also costs O(s) filter tests over the source's s rows), then Algorithm 2: per level, n hashes and n chain pushes of one or two link writes each (each push moves a row from its parent's list to one child list; the tuples stay in the buffer), no allocation per list (#101 B2), one `links` array of 4 B per row allocated zeroed, and one transient table of chains per inner table (24 B a bucket, the root's the largest; the last attribute's table of chains stays as the `Leaf`). Expected cost holds for input in another `HashTrie`'s iteration order, or any subset of it; the bucket-index invariant names the one order it does not cover |
| `from_view` under `incremental` | O(n · a) | O(n · a) | the same `materialize()` copy, then one `insert_at` per row of the copy, in order: the per-row build before #107 |
| `from_view` under `radix:K` | O(n · a + D · K) | O(n · a) | the bulk build over a partitioned copy: a histogram pass over the source's rows and a scatter copying the matching ones (`partition_rows`, one first-attribute hash per tuple, kept for the scatter), a second first-attribute hash per tuple in the scratch roots, and a k-way merge of the D distinct root keys over the 2^K partitions' entry lists (`radix::build_and_merge`, shared with `parallel:N`: O(D · K) heap operations, where a sort, O(D log D), ran until 2026-10-14); transiently the copy beside the view's source while it is made, and 4 B per tuple of `origin` and 2 B of partition number; builds an equivalent trie, the bulk trie's contents and root with the rows in partition order |
| `HashTrieIterator::key()` | O(1) | | array access at the deepest stack entry |
| `HashTrieIterator::next()` | O(1) amortized | | scans forward in the current node's bucket array; per-call amortized constant in practice |
| `HashTrieIterator::lookup(h)` | O(1) expected | | linear probe; O(capacity) worst case |
| `HashTrieIterator::size()` | O(1) | | `HashTable::len()` |
| `HashTrieIterator::open()` | O(1) amortized | | pushes a new stack entry, finds first occupied bucket |
| `HashTrieIterator::open()` into a pruned level | O(1) | | pushes a `Singleton` frame; no table probe, one `H::hash` of the next attribute |
| `HashTrieIterator::open()` into an unexpanded child (lazy) | O(k) first time, O(1) after | O(k) | builds the child's one-level table from its `k` pending tuples, re-threading each row's link into the table's chains through `&self` (`HashTrie::resolve`); later `open`s find it built |
| `insert(tuple)` under `LazyExpansion` | O(1) amortized | O(a) | one root-level hash and probe, then the row added to the child's pending chain (one or two link writes); recurses only into a child a probe has already expanded |
| `HashTrieIterator::up()` | O(1) | | pops the stack |
| `HashTrieIterator::leaf_tuples()` | O(1) | | a `LeafRows` view of the current bucket's chain in its linked form, the chain's head and length over the trie's buffer and links (a pruned level lends the listed form, its one row id); copies nothing. Reading it walks the chain, one link load and one row per step |
| `HeapSize::heap_size_bytes()` | O(node count) | | the buffer (capacity × 8 B) and the links (capacity × 4 B), plus a recursive walk summing each `HashTable` shell (a chain lives in its bucket and owns no heap), and under lazy expansion each unexpanded child's `Box<LazyChild>` (`own_heap_bytes`) and, once built, its table. A tuple is counted once, in the buffer, plus its 4-byte link: at arity 2 that is 16 B plus 4, where a `Vec` per tuple cost 40 B before #111 and each chain added a heap list of 4-byte ids before #101 B2 |
| `for_each_tuple(visit)` | O(n) | O(a) stack | depth-first walk of the trie lending each stored tuple: each chain walked through `links` (a lazy child's pending chain likewise), each pruned `Singleton` through its id, every row read from the buffer; allocates nothing per tuple, so `bench ds` times it (issue #79). `collect_tuples()` copies the buffer, in its row order, and `TupleScan::scan_tuples` scans it in place: neither walks the trie |

The "amortized O(1)" claims assume good hash distribution (no chronic clustering on linear probes). For pathologically bad inputs (e.g., all keys hashing to the same bucket), `lookup` degrades to O(capacity). The hash function is a Layout choice, not a fixed cost: `fxhash` (`FxHashStrategy`) is implemented and selectable with `--ds-layout-hasher fxhash` — see [Layout options](#layout-options). Other non-cryptographic alternatives (`ahash`, AquaHash) remain unimplemented.

## Worked micro-example

Tuples `(1, 2), (1, 3), (2, 4)`, arriving in that order as rows 0, 1 and 2 of the trie's buffer, build (for some specific hash values — the actual hashes depend on the platform's `DefaultHasher`):

```
HashTrie {
  arity: 2,
  tuples: [1, 2, 1, 3, 2, 4],          // rows 0, 1, 2, row-major
  links:  [NIL, NIL, NIL],             // every row is the tail of its chain
  root: Inner(HashTable {
    bucket[i₁]: Some(Entry { hash: h(1), value: Leaf(HashTable {
      bucket[j₁]: Some(Entry { hash: h(2), value: Chain { head: 0, tail: 0, len: 1 } }),   // row 0 = (1, 2)
      bucket[j₂]: Some(Entry { hash: h(3), value: Chain { head: 1, tail: 1, len: 1 } }),   // row 1 = (1, 3)
    })}),
    bucket[i₂]: Some(Entry { hash: h(2), value: Leaf(HashTable {
      bucket[j₃]: Some(Entry { hash: h(4), value: Chain { head: 2, tail: 2, len: 1 } }),   // row 2 = (2, 4)
    })}),
  })
}
```

The build re-threads row 0. Grouping the root starts `h(1)`'s chain at row 0 (link 0 = `NIL`), appends row 1 to it (link 0 = 1, link 1 = `NIL`) and starts `h(2)`'s at row 2 (link 2 = `NIL`). Building `h(1)`'s child walks that chain: it reads row 0's link before regrouping row 0, then starts one chain per second value, which writes `NIL` into links 0 and 1. No list was allocated.

That is the unpruned shape (`--ds-layout-pruning off`, i.e. `HashTrie<H, NoPruning>`, the default). Under `HashTrie<H, SingletonPruning>`, the child for `1` holds two tuples and stays a `Leaf` table, while the child for `2` holds exactly one and collapses to `Singleton(2)`, the row id of `(2, 4)` — so step 6 below pushes a `Singleton` frame instead of a `Table` frame, and its `key()` / `leaf_tuples()` answers are unchanged.

Under `HashTrie<H, NoPruning, LazyExpansion>` (`--ds-layout-expansion lazy`), construction stops at the root: bucket `i₁` holds `Unexpanded { pending: Chain { head: 0, tail: 1, len: 2 } }` and bucket `i₂` holds `Unexpanded { pending: Chain { head: 2, tail: 2, len: 1 } }`, with links `[1, NIL, NIL]`. Step 2 below takes the first child's chain and re-threads its two rows into its `Leaf` table (exactly the table shown above; links become `[NIL, NIL, NIL]`) before pushing the frame, and step 6 builds the second's; the walk's keys and leaf chains are unchanged. A join that never enters `h(2)`'s child never builds it.

Iteration walk (`hash_trie_iter()`):

1. `open()` → stack: `[Table(root, i₁)]`. `key()` returns `h(1)`.
2. `open()` → stack: `[Table(root, i₁), Table(child_for_1, j₁)]`. `key()` returns `h(2)`. `leaf_tuples()` returns a linked `LeafRows` over the chain that starts at row 0, of length 1, whose one row is `(1, 2)`.
3. `next()` → stack deepest: `Table(child_for_1, j₂)`. `key()` returns `h(3)`. `leaf_tuples()` returns a linked `LeafRows` over the chain that starts at row 1, whose one row is `(1, 3)`.
4. `next()` → at end at depth 2. `up()` → stack: `[Table(root, i₁)]`.
5. `next()` → stack: `[Table(root, i₂)]`. `key()` returns `h(2)`.
6. `open()` → stack deepest: `Table(child_for_2, j₃)`. `key()` returns `h(4)`. `leaf_tuples()` returns a linked `LeafRows` over the chain that starts at row 2, whose one row is `(2, 4)`. (Under `SingletonPruning`: a `Singleton` frame at depth 1 over row 2, with the same `key()`, and a `leaf_tuples()` lending the same row in the listed form.)
7. `up()`, `up()`, `next()` → empty stack. Done.

## When to prefer this structure

- Joins where comparison-based seek (LFTJ's least-upper-bound) is the bottleneck — typically when fan-outs are large and the seek's linear sibling scan dominates.
- Joins where the build is a small share of a query: every query builds its own indexes from the catalog's flat tuples (per-query index builds, 2026-10-22), so the build is paid per query, and `bulk` is O(n · a) against the sorted tries' O(n · a · log n) sort.
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
    `P::Payload` is what `HashTrieNode::Singleton` holds: a `RowId` when
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
    kermit-lab. *(2026-10-14, #101 B2: measured while every leaf chain was
    a `Vec`, so the levels pruning removes included a list allocation that
    no build makes since B2; these savings predate it.)*

- **Lazy child expansion** (`ds_layout_expansion`): builds only the root
  table at construction. Every child below it keeps its tuples' row ids as a
  list until a probe first opens it, and then that child's table is built,
  one level at a time (paper §3.3.1, Figure 6). It is a *shape*: an unexpanded
  child is a node state that eager tries must not carry, so it is a Layout.
  - **CLI:** `-i hash-trie --ds-layout-expansion <eager|lazy>` (on
    `kermit join`, `bench join`, `bench run` and `bench ds`).
  - **Choices:**
    - `eager` (default; `EagerExpansion`, every level built at construction)
    - `lazy` (`LazyExpansion`)
  - **Type-level:** `HashTrie<H, P, E: ExpansionPolicy>` (in
    [`expansion.rs`](../../kermit-ds/src/ds/hash_trie/expansion.rs)).
    `HashTrieNode::Unexpanded` holds `E::Pending<Node>`:
    - under `lazy`, a `Box<LazyChild>` with the pending chain in a `Cell`
      (taken when the child is built) and the built table in a `OnceCell`;
    - under `eager`, the uninhabited `Never`, so eager tries keep their node
      and frame sizes (pinned by
      `node_does_not_grow_under_the_expansion_policy` and
      `off_frame_is_the_bare_table_pair`) and stay `Sync`.

    Lazy tries are `!Sync` (still `Send`): a parallel prober would need a
    different cell. Both kinds keep their links as atomics (#101 B2), so
    eager tries stay `Sync`. The payload is generic over the node type rather than
    naming `HashTrieNode`, so the policy traits stay free of crate-private
    types (`private_interfaces`).
  - **Who expands:** only `HashTrieIter::open`, through `HashTrie::resolve`,
    which takes the pending chain and re-threads its rows into the child's
    table through `&self`.
    `for_each_tuple` walks the pending chain, and `heap_size_bytes` counts
    the `Box` (a chain owns no heap); `collect_tuples`, `scan_tuples`,
    `project` and the Parquet round-trip read the buffer; none of them
    expands anything. A `for_each_tuple` visitor that opens an iterator on
    the same trie and expands a child the walk is still reading re-threads
    rows under the walk, which may then lend wrong rows or panic (before
    #101 B2 it panicked with `BorrowMutError`); nothing in kermit does that.
  - **With pruning:** a bucket with one tuple below it is a `Singleton` and
    never expands; two or more make an `Unexpanded` pending chain, the
    evicted singleton tuple first.
  - **Equivalence:** an expanded child is the eager table at that position,
    bucket for bucket, for a trie no `insert` has changed since its build,
    and always under `child-capacity=grow` (see [Invariants](#invariants)).
    Eager and lazy timings therefore compare one variable: when the work is
    done.
  - **Bench methodology:** a lazy family never probes the engine
    `bench run` loaded (`ExecutionFamily::JOIN_MUTATES`). `iteration` builds
    a fresh engine per sample in untimed setup, so each timed join pays its
    own expansion (cold), and `--verify` also runs on a fresh build.
    `space` therefore always measures the relations as built, and
    `run_benchmark` checks their footprint before measuring it.
    `end_to_end` with `--queries-per-build K > 1` shows the amortisation:
    the first query expands and the rest run warm. Eager `iteration`
    reuses one engine, so an eager-vs-lazy `iteration` gap also contains a
    cache-state term; the measurement record bounds it.
  - **Test aliases:**
    - join layer: `HashTrieSipLazy`, `HashTrieFxLazy`,
      `HashTrieSipPrunedLazy`, `HashTrieFxPrunedLazy` (plus one `HalfFull`
      load-factor suite);
    - DS layer: those plus `HashTrieMod10Lazy`, `HashTrieMod10PrunedLazy`
      and `HashTrieSipDenseLazy`.
  - **Bench axis value:** `"eager"` or `"lazy"`.
  - **Measured effect** (2026-10-05, `oxford-uniform-s3 -q triangle`,
    SipHash, AMD Ryzen 7 7700X). Each arm ran 5 replicates in alternating
    order, every invocation behind a quiet gate, and none overlapped a
    contended host sample. Ratios are medians, with the 95 % bootstrap CI
    of the mean ratio in brackets. `insertion`, `space` and `end_to_end`
    cover all 8 of the workload's relations, of which the triangle reads
    the three arity-3 ones (`R`, `S`, `T`). Run directory:
    `kermit-bench-runs/lazy-expansion-2026-10-05/`.
    - *Eager parity* (#92's landing 149614f over the pre-#92 baseline
      35216b6; the span also includes #81's landing):
      `space` is byte-identical on every relation; `insertion` is 0.999×
      [0.994, 1.001] and `iteration` 1.003× [0.990, 1.006], against the
      TreeTrie control's 1.001× and 0.982×. Both are inside the ±3 %
      codegen bound.
    - *Lazy over eager*, pruning off / on (binary 149614f):
      - `insertion`: 0.55× [0.53, 0.55] / 0.73× [0.73, 0.74];
      - as-built `space`, all 8 relations: 0.45× / 0.72×; `R`, `S` and
        `T` alone: 0.19× / 0.44× (deterministic, so no interval);
      - cold `iteration`: 1.68× [1.67, 1.69] / 1.27× [1.25, 1.28];
      - `end_to_end`, K = 1: 1.24× [1.22, 1.24] / 1.07× [1.06, 1.07];
      - `end_to_end`, K = 10: 1.14× [1.14, 1.20] / 1.04× [1.04, 1.05].
    - *Confound bound:* eager `iteration` on the cold path (a build of
      149614f patched to take it, never committed) over the warm path is
      0.989× [0.982, 0.995] with pruning off and 1.009× [1.006, 1.012]
      with it on. That comparison crosses binaries, so the cache-state term
      is indistinguishable from zero inside the ±3 % codegen bound. The
      lazy `iteration` gap is therefore the lazy trie's own cost: its
      expansion plus a slower warm join (next bullet).
    - *Reading:* with pruning off, lazy cuts the build by 45 % and the
      as-built footprint by 55 %, and the first query gives that saving
      back by doing the deferred work. Ten queries per build do not reach parity.
      `end_to_end` times build, queries and drop together, so its K = 10
      minus K = 1 medians are nine warm queries. That puts a warm lazy join
      at about 1.12× eager's with pruning off and 1.03× with it on: the
      expanded lazy trie stays slower than eager on this workload. Pruning
      narrows every gap, consistent with less work being left to defer: a
      bucket with one tuple below it is a `Singleton` under both
      expansions.
    - *Since #107* (2026-10-07, same workload, b9d1d6e over a58317d; see
      [Algorithm 2 A/B](#algorithm-2-ab-107), C): expansion now builds each
      child by Algorithm 2.
      - Lazy `insertion` is 0.93× [0.92, 0.94] with pruning off and 0.91×
        [0.90, 0.91] with it on.
      - Lazy `iteration` is 1.05× [1.04, 1.06] / 1.04× [1.03, 1.07].
      - Eager `iteration` is unchanged, and so is TreeTrie, the control.
      - The numbers above predate #107.
    - *Before #101 B2* (2026-10-14): every number above was measured while
      each pending list and leaf chain was a `Vec`. A pending list is now a
      chain in its child's `LazyChild`, threaded through the trie's links,
      and expansion re-threads it, so lazy `insertion`, `space` and
      `iteration` all predate B2.

### Config flags

- **Load factor cap** (`ds_config_load_factor`): the occupancy a level's
  hash table is allowed to reach before it doubles. This is a *value* read
  on a path the code already takes — the resize comparison in
  `HashTable::entry_or_insert_with`, `(len + 1) * 100 > capacity * percent`
  — so replacing the former compile-time constant with it adds no branch
  and non-users pay nothing. `HashTrie` holds the config and passes the cap
  down through `insert_at` and `group` into the table call; nothing is
  stored per table, so space is unchanged.
  - **CLI:** `-i hash-trie --ds-config load-factor=0.5` (on `bench ds` and
    `bench run`, the two subcommands that also carry the `--ds-layout-*`
    flags). The value is a decimal in the open interval (0, 1) with at most
    two decimal places; anything else is a usage error naming the range.
  - **Default:** `0.7` (the historical constant).
  - **Rust:** `HashTrieConfig { load_factor: LoadFactor::percent(50)? }`
    reaches the trie through
    [`ConfigurableRelation`](../../kermit-ds/src/relation.rs)'s
    `from_view_with_config` (`from_tuples_with_config` is the convenience over the identity view); `LoadFactor` is an exact percentage in
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
- **Root capacity** (`ds_config_root_capacity`): how large a build makes
  the root table. Under `grow` (the default) the root starts at 4 buckets
  and doubles as keys arrive, like every other table. Under `tuples`, a
  trie built from a known set of n tuples sizes its root once, at the
  smallest power of two ≥ 4 with `n · 100 ≤ capacity · percent`, where n is
  `view.count()`, the rows the filter keeps. Distinct
  keys cannot outnumber tuples, so the root never grows during that build.
  This is Algorithm 2, line 3 of the paper (`2^⌈log2(1.25·|L|)⌉`) applied
  to the root: at `load-factor=0.8` the two agree exactly, except that the
  paper gives 2 buckets for one tuple. It is a *value* on a path every
  build takes (the root's starting capacity, read once per trie), so the
  default pays nothing for it. Child tables are sized by `child-capacity`,
  below.
  - **CLI:** `-i hash-trie --ds-config root-capacity=tuples` (combinable:
    `--ds-config load-factor=0.8,root-capacity=tuples`). Any other value is
    a usage error naming `grow` and `tuples`.
  - **Default:** `grow` (the only behaviour before #88).
  - **Rust:** `HashTrieConfig { root_capacity: RootCapacity::Tuples, ..HashTrieConfig::default() }`.
    Every constructor that is given a view presizes, from `view.count()`: the `bulk`,
    `incremental`, `radix:K`, `parallel:N` and `presized:N` builds (`presized:N` requires
    `tuples`; see [Build modes](#build-modes)), `project`, `Configured`, and
    the bench families through `RelationFamily::build_index`. A trie created empty (`new`, `with_config`)
    starts at 4 buckets, and an `insert` after a build may still grow the
    root. All of them size the root by `HashTrieConfig::root_log2_capacity`.
  - **Bench axis value:** the JSON string `"grow"` / `"tuples"`.
  - **Expected effect:** `build` (`insertion` before schema v5) falls when the first attribute has
    many distinct values, because the root skips every rehash. `space` rises
    when tuples outnumber distinct first values, because the root is up to
    n/D times the grown one (`heap_size_bytes` counts every bucket).
    `iteration` may slow at the root then, because there are more empty
    buckets to skip. When every first value is distinct, the capacity equals
    the grown one, and only slot placement differs. Unmeasured as of this
    writing.
- **Child capacity** (`ds_config_child_capacity`): how large a build makes
  every table below the root. Under `grow` (the default) a child starts at
  4 buckets and doubles as keys arrive. Under `tuples` each child is sized
  once from |L|, the length of the list Algorithm 2 builds it from: the
  smallest power of two ≥ 2 with `|L| · 100 ≤ capacity · percent`, which at
  `load-factor=0.8` is the paper's `2^⌈log2(1.25·|L|)⌉` exactly, one-tuple
  lists included. Distinct keys cannot outnumber tuples, so no child grows
  during the build. It is a *value* on a path every `bulk` build takes (a
  child's starting capacity, read once per child), so the default pays
  nothing for it. Lazy expansion sizes an expanded child from its pending
  list the same way.
  - **CLI:** `-i hash-trie --ds-config child-capacity=tuples`. The paper's
    sizing throughout is `--ds-config
    root-capacity=tuples,child-capacity=tuples,load-factor=0.8`.
  - **Default:** `grow` (the only behaviour before #107).
  - **Prerequisite:** `--ds-build hash-trie=incremental` requires
    `child-capacity=grow`: the per-tuple build creates a child on its first
    tuple, before the child's list is known.
  - **Rust:** `HashTrieConfig { child_capacity: ChildCapacity::Tuples, ..HashTrieConfig::default() }`;
    `HashTrieConfig::child_log2_capacity`. After the build, a child that
    `insert` creates starts at 4 buckets under eager expansion; under lazy
    expansion it is sized from its pending list when first probed.
  - **Bench axis value:** the JSON string `"grow"` / `"tuples"`.
  - **Expected effect:** `build` (`insertion` before schema v5) falls where children would rehash as
    they grow. `space` rises where a child's list holds more tuples than
    distinct keys (|L| > D), the tuples-versus-keys question #113 raises
    for the root, and falls for one-tuple children with pruning off: at a
    load factor of 50 % or more `tuples` gives them 2 buckets against
    `grow`'s 4 (`log2_capacity_for(1, …)` with the paper's 2-bucket minimum),
    which is common in sparse binary relations.
  - **Measured effect** (2026-10-07, [Algorithm 2 A/B](#algorithm-2-ab-107),
    under `root-capacity=tuples`):
    - Under `bulk`, `insertion` is 0.95× `grow` by geomean, and 0.86–0.94×
      on the binary relations.
    - Under `presized:16` it is unchanged (0.99×; measured before
      `presized:N` copied its rows into partition order, #101 B1).
    - `iteration` and `space` are unchanged except on `price`, whose
      one-tuple children take 2 buckets: `iteration` is 0.83× there and
      `space` 0.82×.
    - The predicted rise did not appear: these relations are sets, so a
      child's |L| equals its D.
    - *(2026-10-14, #101 B2: measured while every list was a `Vec`; no
      list allocates since B2, so these `insertion` ratios predate it.)*

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

### Build modes

Every mode here builds an equivalent trie under the same config, under
every config it accepts: the same contents, the same capacities, the same
`heap_size_bytes`. That is the BuildMode rule since Amendment 2
(2026-10-06; issues #91, #94, #101, #107). The modes differ only in what
the rule leaves free:
- `bulk` and `incremental` build the identical trie, bucket for bucket and
  id for id.
- `radix:K`, `parallel:N` and `presized:N` copy their rows into partition
  order and keep the copy as the trie's buffer (#101 B1, 2026-10-08). Every
  chain, singleton and pending list holds the same tuples, in the same
  order, under other row ids.
- `radix:K`'s and `parallel:N`'s tables are `bulk`'s bucket for bucket, the
  root included; `presized:N` may also place root keys in other buckets.

So `iteration` is measured for the three partitioning modes, never assumed.

Every mode reads the same `TupleView` and copies its matching rows exactly once, so the build metric (`build` since schema 5) times that copy in every mode alike. Before per-query index builds (2026-10-22), `bulk` and `incremental` kept the caller's batch and only the partitioning modes' copy sat inside the timer, which skewed every build-mode comparison (#91, #94, #101). The measurements recorded below predate this change: their `insertion` is the old definition and is not comparable with `build`. Every buffer is created by its own build at exact size, so the buffer adds the same bytes to `heap_size_bytes` in every mode.

| Mode | `--ds-build` | Method |
|---|---|---|
| `Bulk` (default) | `hash-trie=bulk` | Algorithm 2 (§3.2.2): each table's tuples grouped into its buckets, then each bucket's child built from its list ([Construction](#construction)) |
| `Incremental` | `hash-trie=incremental` | one `insert_at` per tuple, in input order: the build named `serial` before #107; requires `child-capacity=grow` |
| `Radix(K)` | `hash-trie=radix:K`, K in 1..=16 | copy the rows into 2^K partitions on the top K bits of the first attribute's hash and keep the copy as the buffer (SIGMOD 2020 §3.3.2: the tuples partitioned as they are materialised; one round rather than the paper's two passes is kermit's, #101), group each partition into a scratch root and build its children by Algorithm 2, merge the scratch roots (kermit's) |
| `Parallel(N)` | `hash-trie=parallel:N`, N in 1..=1024 | the radix build's partition step (the copy) and build step on N threads (P = 4·N partitions, rounded up to a power of two), then a k-way merge into the root on the calling thread (morsel-driven partitioning and materialisation per §3.3.2; the scratch-root build and the merge are kermit's) |
| `Presized(N)` | `hash-trie=presized:N`, N in 1..=1024; **requires `--ds-config root-capacity=tuples`** | the input copied into partition order by the first attribute's hash (§3.3.2, #101), one partition per run of regions of the presized root, each worker grouping its regions (Algorithm 2, lines 4–7), a tail of deferred tuples pushed on the calling thread in input order, then the children built a run of buckets at a time by whichever worker is free; the regions, the tail and the parallel recursion are kermit's (Amendment 3 of the optimisation standard) |

**The radix build** ([`radix.rs`](../../kermit-ds/src/ds/hash_trie/radix.rs)):

1. **Partition.** `morsel::partition_rows`, on the calling thread with the
   whole source as one morsel, copies the view's matching rows into one of
   2^K partitions by the top K bits of its first attribute's hash: a
   histogram pass, which tests `view.matches(row)` and keeps each matching
   row's partition number, then a stable scatter that calls `view.write`
   into one new exact-size buffer, recording each copied row's source row
   id (`origin`, its input position). The copy becomes the trie's buffer,
   so partition p is the contiguous id range `offsets[p]..offsets[p + 1]`,
   in input order (#101). The view's source is only read. One morsel,
   because `partition_rows` keeps a count and a range per (partition,
   morsel) pair: at `radix:16` over 10⁷ rows, 16 384-row morsels would make
   about 40 million pairs.
2. **Scratch roots.** Each non-empty partition's ids are grouped into a
   scratch root of the real root's kind (Algorithm 2, lines 4–7), recording
   the input position (`origin[id]`) of the row that introduced each key;
   then each key's child is built from its list, as the bulk build builds
   it, reading only that partition's rows.
3. **Merge.** Every scratch entry moves into the real root, in the order of
   those first-appearance positions.

**Why it is equivalent.**

- A table's final layout depends only on the order in which its *new* keys
  arrive, because `entry_or_insert_with` returns an existing entry before
  its resize check.
- The merge inserts the root's distinct keys in first-appearance order, as
  the bulk build does. It orders them by input position, not by id, since
  ids in the copy run partition by partition, so the root is `bulk`'s,
  bucket for bucket.
- Each root key's child is built from the same tuples, in input order,
  because the copy is stable. That covers every table, `Singleton`, chain
  and capacity and, under `LazyExpansion`, every pending list. Only the ids
  differ: a chain names its tuples by their positions in the copy.

**Cost.** Every per-tuple probe and every child build stays inside one
partition, about 1/2^K of the trie and one contiguous range of the buffer,
and only the D merge inserts (one per distinct first-attribute hash) touch
the real root at random. In return the build pays for:

- the copy: a histogram pass over the source's rows and a scatter of the matching ones, into buffers
  allocated zeroed (safe Rust cannot hand out uninitialised ranges; the
  zeroing costs no serial memset when the allocator hands out fresh pages,
  as it does for a large first allocation, though a reused allocation may
  still be zeroed by the allocator);
- a second hash of each tuple's first attribute (`partition_rows` hashes
  each row once and keeps the number for its scatter);
- the k-way merge of the D merge entries over the 2^K partitions' entry
  lists, `parallel:N`'s merge (`radix::build_and_merge` serves both; a sort
  of all D entries until 2026-10-14);
- transient memory: the copy beside the view's source while it is made, 2 bytes
  per tuple of partition number (between the two passes), 4 bytes per
  tuple of `origin` (freed before the merge, once the scratch roots hold
  each key's first input position; `presized:N` frees it after its tail),
  and the scratch tables. The scratch roots' chains are threaded through
  the trie's own `links` and move into the real root unchanged, since
  their ids are global (#101 B2).
  Before #101 B1 the partitions held 2 bytes of partition number and 4 of
  row id per tuple in lists instead of a copy of the rows, and 32 bytes of
  `(index, tuple)` pair before #111.

**The #66 input shape.** A scratch root receives keys that share their top
K hash bits. The per-capacity multiplier turns that shared prefix into one
constant added to every key's product, so the partition does not cluster.
`absorbing_one_radix_partition_costs_no_more_than_unrestricted_keys`, in
[`hash_table.rs`](../../kermit-ds/src/ds/hash_trie/hash_table.rs), pins this:
without the multiplier, one partition costs about 90× the probes.

**The parallel build** (`parallel.rs`) runs the radix build's partition
step through `morsel::partition_rows` and its build step through
`morsel::dispatch`, so each runs on N threads: N workers make the copy,
each scattering its morsels' rows into disjoint ranges of the one new
buffer. The calling thread then merges the partitions' entries into the
root by a k-way merge on their first-appearance positions (`origin`). The
trie holds the radix build's contents and root, and so the bulk build's.
Steps, equivalence argument, complexity and a worked example:
[`parallel-build.md`](./parallel-build.md#hashtrie).

**The presized build** (`presized:N`). It requires `root-capacity=tuples`,
since its regions are cut from a root sized before any tuple arrives; the
CLI rejects the pair otherwise and the constructor panics. It copies the
input into partition order by the first attribute's hash (§3.3.2, #101),
one partition per contiguous run of regions of the presized root, and each
worker appends every row of its partition, one contiguous range of the
copy, to its bucket's chain, threading the row's link. Keys whose probe would cross their
region's end are finished by the calling thread, in a tail, in input order
(by `origin`: ids in the copy run partition by partition, and the
partition count depends on N). The paper does not say how it handles
that case, so this is kermit's answer. After the tail, the children are
built by Algorithm 2 a run of buckets at a time, the runs handed to
whichever worker is free (how the recursion is spread over threads is
kermit's choice; the paper is silent). A run's worker re-threads the rows
of its buckets' chains, including a deferred key's rows, which may lie in
another run's partition; a row is in one list at a time, so each link
still has one writer (#101 B2). `origin` and the partition offsets
are freed right after the tail, before the children are built. The trie is
equivalent to bulk's (Amendment 2): every subtrie and chain holds the same
tuples in the same order; the root has the same capacity, the same
occupied buckets and the same total displacement; and its contents are the
same for every N, bucket for bucket, though each N's buffer is its own
partitioned copy. The
closest-to-paper configuration is
`--ds-config root-capacity=tuples,child-capacity=tuples,load-factor=0.8
--ds-build hash-trie=presized:N`, with pruning and lazy expansion on.
Details:
[`parallel-build.md`](./parallel-build.md#the-presized-build-presizedn).

- **Axis:** `ds_build_mode` (`bulk` / `incremental` / `radix:K` / `parallel:N` /
  `presized:N`), on every HashTrie report. The bench family that ran the build
  emits it, because the trie cannot tell how it was built. kermit-lab reads a
  HashTrie row without the axis, or with the pre-#107 `serial`, as
  `incremental`.
- **API:** `HashTrieBuildMode`, through
  `BuildModeRelation::from_view_with_build_mode`. To set a Config value
  as well, use `HashTrie::from_view_with_config_and_build_mode`.
  `Relation::from_view` uses `Bulk`. `HashTrieBuildMode::Presized` needs
  a `root-capacity=tuples` config, so build it with
  `HashTrie::from_view_with_config_and_build_mode` (or
  `BuiltWith<Configured<…>, …>`); `BuildModeRelation::from_view_with_build_mode`
  uses the default config and panics for it. Each `from_tuples*` twin is
  the convenience over `TupleView::all`: it takes the tuples, builds the
  identity view and calls the `from_view*` method.
- **Tests:**
  - `bulk_builds_the_incremental_trie_*` and
    `tuples_sizes_every_child_from_its_list` in `bulk.rs`, the
    `HashTrieSipIncremental` aliases, and the `HashIncremental`,
    `SizedChildren` and `PaperSizing` join suites.
  - `bulk_and_incremental_thread_the_same_links` in `bulk.rs` (the two
    builds' `links` arrays are equal, #101 B2); the `chain.rs` tests
    (`start` and `push` keep input order, every chain ends in `NIL`,
    `rethreading_a_chain_visits_every_row`) and its size pin of the 24-byte
    leaf bucket; `resolve_rethreads_the_pending_chain_through_a_shared_reference`
    in `implementation.rs` (lazy expansion re-threads through `&self`); and
    `emit_leaf_resets_each_cursor_on_carry` in `hash_triejoin.rs` (the leaf
    product over chains of lengths 1, 2 and 3).
  - `radix_builds_the_bulk_trie_*` in `radix.rs`: content-level identity
    (`identity::assert_same_contents`: every table bucket for bucket, the
    root included; every chain, singleton and pending list by the tuples
    its ids name; the buffers as a multiset at the same capacity) and equal
    heap size, across arity, pruning, expansion, hasher, K, load factor and
    input. Miri runs a smaller matrix.
    `radix_keeps_the_partitioned_copy_as_its_buffer` pins the buffer to
    `morsel::partition_rows`' output, the one difference no content test
    sees (#101).
  - The `HashTrieSipRadix2` and `HashTrieSipLazyRadix2` aliases in
    `kermit-ds/tests/hash_trie_tests.rs` (and `HashTrieSipRadix2` in
    `parquet_tests.rs`).
  - `define_multiway_join_test_suite_for_build_mode!` with `Radix2` in
    `kermit/tests/join_tests.rs`, on Sip/off/eager, Fx/on/eager and
    Sip/on/lazy, under every optimiser.
  - `parallel_builds_the_bulk_trie_*` and `build_modes_reach_their_builds`
    in `parallel.rs`: content-level identity for N ∈ {1, 2, 3, 8}, morsels
    of 7 and of 16 384, arity 1–4 and a dominant key; and the record that
    shows which build ran, with which N, and its worker runs.
    `parallel_stores_its_partitioned_copy` pins the buffer to
    `partition_rows`' output at `partition_bits(N)` partitions.
  - The `HashTrieSipParallel2`, `HashTrieSipLazyParallel2` and
    `HashTrieFxPrunedParallel2` aliases in `hash_trie_tests.rs` (and
    `HashTrieSipParallel2` in `parquet_tests.rs`), and
    `define_multiway_join_test_suite_for_build_mode!` with `HashParallel2`
    in `join_tests.rs`.
  - `presized_parallel_builds_are_equivalent_*` and
    `presized_parallel_builds_are_the_same_for_every_n_on_dense_roots` in
    `parallel.rs`: equivalence with bulk under `root-capacity=tuples`
    (children compared by contents) and content identity with
    `presized:1`, for every Layout, load factors 50–95 % and
    N ∈ {1, 2, 3, 8}, at 8-bucket regions and at the real size;
    `presized_stores_its_partitioned_copy` (the buffer is
    `partition_rows`' copy by home-bucket run);
    `presized_tail_goes_in_input_order_for_every_n` (the tail is pushed by
    `origin`, so a tail key's bucket is the same at N = 1 and N = 4);
    `keys_that_cannot_fit_their_region_go_to_the_tail` (a hash that homes
    every key at a region's last bucket defers 12 of 16 tuples); and
    `one_run_of_grouping_builds_the_bulk_root`, the grouping step without
    threads.
  - `presized_build_reaches_its_own_path` and
    `parallel_build_merges_under_every_root_capacity` in `parallel.rs`: each
    mode reaches its own fill, and `parallel:N` holds bulk's contents and
    root under both root capacities; `presized_build_requires_a_presized_root`
    (`parallel.rs`), `every_prerequisite_is_reachable` and
    `ds_choices_resolve_rejects_a_violated_prerequisite`
    (`kermit/src/options.rs`) and `cli_rejects_presized_without_a_presized_root`
    (`kermit/tests/cli_hash_trie_build_mode.rs`): the prerequisite at both
    boundaries.
  - The `HashTrieSipPresized2`, `HashTrieSipLazyPresized2` and
    `HashTrieFxPrunedPresized2` aliases in `hash_trie_tests.rs` (and
    `HashTrieSipPresized2` in `parquet_tests.rs`), built with `HashPresized2`,
    and the presized `define_multiway_join_test_suite_for_build_mode!`
    invocations in `join_tests.rs`; `built_with_stacks_on_configured`
    (`kermit-ds/src/configured.rs`) pins that the stacked markers reach the
    presized fill.
  - `hash_trie_families_build_with_their_parallel_mode` in
    `kermit/src/execution.rs` (the mode reaches the build on every route,
    and `presized:N` reaches the presized fill) and
    `kermit/tests/cli_hash_trie_build_mode.rs`.
  - `partition_rows_*` in `kermit-ds/src/morsel.rs`: offsets, membership,
    stability, `origin`, the same copy for every thread count and morsel
    size, the copy's exact capacity, one `partition_of` call per row and two
    worker runs. The `identity.rs` self-tests show that the content
    comparison accepts a reversed buffer and rejects a row that differs
    while hashing alike.
  - kermit-ds's view tests (`parallel.rs`): `parallel:N` over a filtered,
    permuted `TupleView` builds the trie `bulk` builds from that view, in
    contents and capacities (`identity::assert_same_contents`); `presized:N`
    builds an equivalent one (`assert_equivalent_trie`). `relation.rs`'s
    `from_view_builds_what_the_materialised_batch_builds` checks each
    structure's default build against its materialised batch.
- **When `incremental` is useful:** reproducing pre-#107 `insertion`
  numbers, and measuring what Algorithm 2's order buys over the per-tuple
  descent (the locality #101 is about).
- **Measured effect of `bulk`** (2026-10-07, [Algorithm 2 A/B](#algorithm-2-ab-107)):
  over `incremental`, `insertion` depends on the input's shape.
  - Faster where first keys repeat in random order: 0.80× on `binary-1e7`
    and 0.89× on `binary-1e6`.
  - About even on unary relations and on shuffled `friendof` (0.96–0.99×).
  - Slower where the input arrives grouped by first key (1.19× on
    `friendof`) and where every first key is distinct (1.62× on `price`).
  - The geomean over 13 relations is 1.02×. `space` is identical.
  - *(2026-10-14, #101 B2: measured while every list was a `Vec`. B2
    threads the lists through per-row links, which removes the per-list
    allocations #107's reading blames for the `price` ratio, so these
    ratios predate it.)*
- **Measured effect, earlier modes:** *(2026-10-07, #107: every arm in these records built
  its subtries per tuple, by `insert_at`. `serial` is the per-tuple build,
  `incremental` since #107, and `radix:K`, `parallel:N` and the presized
  arms (then spelled `parallel:N`) built that way too. Today's modes of
  those names group first and build children by Algorithm 2, so none of
  these numbers measures a post-#107 mode. 2026-10-08, #101 B1: they also
  partitioned row ids and left the rows in arrival order, where today's
  `radix:K`, `parallel:N` and `presized:N` copy the rows into partition
  order first. 2026-10-14, #101 B2: and every chain was a `Vec` of its own,
  where today's are threaded through per-row links.)* On inputs that arrive grouped
  by their first attribute, slower single-threaded, with identical space:
  1.12–1.14× `serial`'s `insertion` time on `friendof` and 1.93–2.28× on `price`. On
  `friendof` with its rows shuffled, `radix:12` is 0.90× `serial`, the only
  arm that wins. See [Radix build A/B](#radix-build-ab) and
  [its shuffled-input run](#radix-build-ab-shuffled-input).
  `parallel:N` was measured by #94's scaling run of 2026-10-06 (summarised
  in `docs/specs/2026-10-06-hash-trie-presized-parallel-build-design.md`,
  § Motivation). The presized build's curve (spelled `parallel:N` under
  `root-capacity=tuples` when it was run), against the default config on
  the same jemalloc binary, is in
  [`parallel-build.md`](./parallel-build.md#scaling-result-hashtrie-presized-and-grown-2026-10-06):
  where first keys rarely repeat it reaches 2.2–2.7× at `:16`, where the
  grown build stays at about 1×; where they repeat, tuple-count sizing makes it
  slower than the grown build.

#### Radix build A/B

> **2026-10-07 (#107):** Every arm in this record built its subtries per tuple, by `insert_at`. `serial` is the per-tuple build, `incremental` since #107, and the `radix:K` arms built that way too. Today's `radix:K` groups first and builds its children by Algorithm 2, and the default is now `bulk`, so none of these numbers measures a post-#107 mode.

> **2026-10-08 (#101 B1):** These arms also partitioned row ids and left the rows in arrival order. Today's `radix:K` copies the rows into partition order (`morsel::partition_rows`) and keeps the copy as the trie's buffer, so each partition's build reads one contiguous range, and its `insertion` includes the copy. These numbers do not measure a post-B1 `radix:K` either.

> **2026-10-14 (#101 B2):** These arms also kept every leaf chain as a `Vec` of its own. Today's chains are threaded through one 4-byte link per row (`ChainLinks`), so no list allocates and `space` counts the links instead; these numbers do not measure a post-B2 build.

`bench ds -m insertion space` on two relations from the WatDiv cache
`watdiv-stress-100-test-1`, on 2026-10-05. The binary was built at 596f218,
the #91 landing (sha256 `f92e8ebbbd370045…`), and ran on an AMD Ryzen 7
7700X with the default Layout (SipHash, pruning off, eager). Each arm ran
5 replicates at `--sample-size 10`: odd replicates in the order serial,
radix:4, radix:8, radix:12, and even ones in reverse. Every invocation
waited for a quiet host; the one that overlapped a peer session's build
was re-run. Times are the median, with the min–max, of the per-replicate
Criterion mean. Ratios are medians over `serial`, with the 95 % bootstrap
CI of the mean ratio in brackets. Run directory:
`kermit-bench-runs/radix-ab-2026-10-05/`.

| Relation | n | D (distinct first-attribute keys) | D/n |
|---|---|---|---|
| `friendof` | 4,491,142 | 39,781 | 0.009 |
| `price` | 240,000 | 240,000 | 1.000 |

| Relation | Arm | `insertion` (ms), median [min, max] | Over `serial` |
|---|---|---|---|
| `friendof` | `serial` | 271.9 [270.2, 273.8] | 1 |
| | `radix:4` | 311.3 [309.6, 312.1] | 1.14× [1.14, 1.15] |
| | `radix:8` | 307.1 [306.6, 311.2] | 1.13× [1.13, 1.14] |
| | `radix:12` | 305.8 [305.4, 307.9] | 1.12× [1.12, 1.13] |
| `price` | `serial` | 26.8 [26.7, 27.3] | 1 |
| | `radix:4` | 61.1 [60.6, 61.6] | 2.28× [2.25, 2.29] |
| | `radix:8` | 51.7 [51.2, 52.1] | 1.93× [1.90, 1.94] |
| | `radix:12` | 60.3 [59.9, 60.5] | 2.25× [2.22, 2.25] |

`space` is identical across all four arms: 797,992,480 bytes for
`friendof` and 86,960,128 for `price`, as the identity rule requires.

**Reading.** Single-threaded, the radix build is slower than `serial` on
both relations at every K. Both inputs already arrive grouped by their
first attribute. `friendof` is 39,781 runs, one per distinct key (grouped,
though not sorted), and `price` is sorted with every key distinct. The
serial build therefore already makes each subtrie's inserts in one burst,
which is the locality radix partitioning exists to create, so here the
partition passes are pure overhead. On `price` (D = n) the merge also makes
as many random root inserts as the serial build does. SIGMOD 2020
§3.3.2 partitions for cache locality inside a parallel, morsel-driven
build. This A/B measures one thread only; parallel builds are #94.

#### Radix build A/B, shuffled input

> **2026-10-07 (#107):** Every arm in this record built its subtries per tuple, by `insert_at`. `serial` is the per-tuple build, `incremental` since #107, and the `radix:K` arms built that way too. Today's `radix:K` groups first and builds its children by Algorithm 2, and the default is now `bulk`, so none of these numbers measures a post-#107 mode.

> **2026-10-08 (#101 B1):** These arms also partitioned row ids and left the rows in arrival order. #101's locality run (2026-10-08, `kermit-bench-runs/hash-trie-locality-2026-10-08/`) located the order penalty this record found in reads of tuple memory. Today's `radix:K` copies the rows into partition order first, so the shuffled column is the case #101 B1 targets; these numbers do not measure it.

> **2026-10-14 (#101 B2):** These arms also kept every leaf chain as a `Vec` of its own. Today's chains are threaded through per-row links beside the buffer, so a build allocates per table, never per list; these numbers do not measure a post-B2 build.

The paper's ablation (§5.4.2 of the technical report TUM-I2082) calls radix
partitioning "arguably the most important optimization", because "it eliminates any
runtime fluctuations due to the specific order in which data is stored in
the base tables". Its gains appear from 500M edges up: read as the ratio of
Table 6's last two columns, removing radix costs 1.13× at 500M edges and
1.20× at 1.2B, and nothing at 5M or 50M. To test the order claim, the A/B
above was repeated on `friendof` in two orders:
- *grouped:* the cached file;
- *shuffled:* its rows permuted with seed `0x91`, giving the same schema
  and tuples in 4,490,997 runs instead of 39,781.

The two inputs and four arms were interleaved in each replicate, with both
orders alternating, using the same binary and protocol as above. Three
steps that overlapped a peer session's work were re-run. Run directory:
`kermit-bench-runs/radix-shuffle-2026-10-05/`.

| Arm | Grouped (ms) | Shuffled (ms) | Over `serial`, shuffled | Shuffled over grouped |
|---|---|---|---|---|
| `serial` | 274.3 [272.7, 277.3] | 764.3 [759.0, 784.6] | 1 | 2.79× [2.77, 2.84] |
| `radix:4` | 313.8 [311.9, 316.7] | 868.9 [859.8, 887.1] | 1.14× [1.12, 1.15] | 2.77× [2.75, 2.81] |
| `radix:8` | 310.0 [308.4, 314.9] | 808.4 [805.0, 821.0] | 1.06× [1.04, 1.07] | 2.61× [2.59, 2.63] |
| `radix:12` | 310.0 [306.6, 312.7] | 689.1 [685.5, 692.5] | 0.90× [0.88, 0.91] | 2.22× [2.21, 2.24] |

`space` is 797,992,480 bytes in every cell. The grouped column reproduces
the first A/B: radix is 1.13–1.14× `serial`.

**Reading.** Input order matters far more than the build mode: shuffling
the same tuples makes the serial build 2.79× slower. On the shuffled input
radix helps more as K grows, and at `radix:12` it beats `serial` by 10 %,
the only case where radix wins at all. It does not remove the order
penalty, though. `radix:12` cuts it from 2.79× to 2.22×. What makes up the
remaining penalty was not measured.

#### Algorithm 2 A/B (#107)

> **2026-10-14 (#101 B2):** Every list in this record was a `Vec<RowId>`: each bucket's list at every level of `bulk`'s grouping, each leaf chain and each lazy pending list was a heap allocation of its own, the cost the *All keys distinct* reading below names. Today's lists are chains threaded through one 4-byte link per row, so `bulk` allocates per table, never per list. These `insertion`, `space` and `iteration` numbers do not measure a post-B2 HashTrie; TreeTrie's do.

The replicated measurement of #107, run 2026-10-07 after the landing, on an
AMD Ryzen 7 7700X with jemalloc. Run directory:
`kermit-bench-runs/hash-trie-algorithm-2-2026-10-07/` (README, `run.sh`,
`analyse.py`, `analysis.txt`). It used two binaries, each built from a
`git archive` in its own target directory with one toolchain:
- NEW is b9d1d6e, the landing (sha256 `640d3436d8c8f0da…`).
- OLD is a58317d, master just before it (sha256 `71388ef3fb6e6065…`).

**Method.**
- Each arm ran 5 replicates, with the arm order rotated per replicate.
- Every invocation waited behind a quiet gate and a memory guard.
- Times are the median of the per-replicate Criterion means. Ratios are
  medians, with the 95 % bootstrap CI of the mean ratio in brackets.
- 42 of the 660 steps overlapped a busy host sample (another session's
  work). They are left out and were not re-run, so some cells rest on fewer
  than 5 replicates (`n` in `analysis.txt`). `binary-1e7` in part A has 2
  `incremental` and 3 `bulk`.
- Compare these numbers within this record only: the radix records above
  ran on a glibc binary (596f218, before #112).

**A. `bulk` over `incremental`** (NEW binary; `bench ds -m insertion`;
default config and Layout). The inputs are the 2026-10-06 runs' 12
relations plus `friendof` shuffled with seed `0x91`:
- `binary-N` has about 10 tuples per first key, in random order.
- `unary-N` is a single random column.
- `friendof` and `price` are as in the radix A/B: grouped, and all keys
  distinct.

| Relation | `incremental` (ms) | `bulk` (ms) | `bulk` ÷ `incremental` |
|---|---|---|---|
| `unary-1e3` | 0.025 | 0.024 | 0.96× [0.94, 1.00] |
| `binary-1e3` | 0.046 | 0.049 | 1.07× [1.05, 1.10] |
| `unary-1e4` | 0.315 | 0.305 | 0.97× [0.93, 0.98] |
| `binary-1e4` | 0.616 | 0.625 | 1.02× [1.00, 1.03] |
| `unary-1e5` | 4.34 | 4.29 | 0.99× [0.94, 1.01] |
| `binary-1e5` | 7.89 | 7.93 | 1.01× [0.98, 1.03] |
| `unary-1e6` | 85.0 | 82.2 | 0.97× [0.93, 1.00] |
| `binary-1e6` | 174.7 | 155.7 | 0.89× [0.84, 0.95] |
| `price` | 41.5 | 67.2 | 1.62× [1.59, 1.72] |
| `friendof` | 282.0 | 334.5 | 1.19× [1.15, 1.25] |
| `unary-1e7` | 1,473 | 1,434 | 0.97× [0.95, 1.01] |
| `binary-1e7` | 2,826 | 2,260 | 0.80× [0.79, 0.84] |
| `friendof`, shuffled | 764 | 736 | 0.96× [0.92, 1.00] |

The geomean over the 13 is 1.02×. `space` is identical between the two
builds on every relation whose `space` replicate was clean (10 of 10), as
the BuildMode identity requires.

**C. NEW over OLD** (`bench run oxford-uniform-s3 -q triangle`). The
Criterion settings and workload are those of the lazy-expansion record
above. `insertion` is summed over the workload's 8 relations. The default
build changed between the two binaries, from per-tuple to `bulk`.
TreeTrie's code did not change, so its ratio is the codegen bound.

| Layout | `insertion` | `iteration` |
|---|---|---|
| pruning off, eager | 1.15× [1.14, 1.16] | 1.00× [0.99, 1.01] |
| pruning off, lazy | 0.93× [0.92, 0.94] | 1.05× [1.04, 1.06] |
| pruning on, eager | 1.09× [1.08, 1.09] | 1.00× [0.99, 1.00] |
| pruning on, lazy | 0.91× [0.90, 0.91] | 1.04× [1.03, 1.07] |
| TreeTrie/LFTJ (control) | 1.00× [1.00, 1.01] | 0.99× [0.97, 0.99] |

`space` is identical across the binaries in all five cells.

**B. `child-capacity=tuples` over `grow`** (NEW binary;
`bench ds -m insertion iteration`, `space` in the first replicate). Every
cell ran with `root-capacity=tuples`, so only the children differ: `bulk`
and `presized:16`, each at load factors 0.7 and 0.8.

> **2026-10-08 (#101 B1):** The `presized:16` cells built over the input in arrival order. Today's `presized:N` copies its rows into partition order first, so those cells do not measure a post-B1 build; the `bulk` cells do.

| Cell | `insertion`, geomean | `friendof` | `binary-1e7` | `price` | `iteration`, geomean |
|---|---|---|---|---|---|
| `bulk`, 0.7 | 0.95× | 0.91× [0.85, 0.94] | 0.93× [0.91, 0.94] | 0.97× [0.96, 0.97] | 0.99× |
| `bulk`, 0.8 | 0.95× | 0.88× [0.84, 0.88] | 0.94× [0.92, 0.95] | 0.97× [0.96, 1.00] | 0.99× |
| `presized:16`, 0.7 | 0.99× | 0.99× [0.98, 1.00] | 1.00× [0.99, 1.01] | 0.97× [0.97, 0.98] | 0.99× |
| `presized:16`, 0.8 | 0.99× | 1.00× [0.99, 1.01] | 0.99× [0.98, 1.03] | 0.97× [0.97, 0.98] | 0.98× |

- Under `bulk`, the other binary relations move by 0.86–0.93×. The unary
  ones move by 0.99–1.02×; they have no children.
- `iteration` is within 2 % of `grow` everywhere except `price`, where it
  is 0.83–0.84× in every cell.
- `space` is 1.000× `grow` everywhere except `price`, at 0.82×, and
  `binary-1e3`, at 1.002×.

**Reading.**
- **Order and grouping.** Algorithm 2's grouping pays where the input gives
  each first key no locality: shuffled order with repeated keys
  (`binary-1e6`, `binary-1e7` and shuffled `friendof`). It costs where the
  per-tuple descent already had that locality, as in grouped `friendof`,
  the same finding as the radix A/B.
- **All keys distinct.** On `price` every list holds one tuple. The build
  then allocates and frees a one-tuple list per key, which the per-tuple
  descent never makes. Umbra threads its lists through an 8-byte chain
  pointer in each tuple (§3.3.2), so the paper pays no such cost. In
  kermit it is the price of a `Vec` per bucket (#101, #111).
- **Unary relations.** Here the root is the leaf, so the two builds do the
  same work.
- **Small inputs.** Where everything fits in cache, grouping's locality
  buys nothing and only its extra pass shows: `binary-1e3` is 1.07×, and
  `oxford-uniform-s3`'s small relations build 1.09–1.15× slower eagerly
  (C).
- **Lazy expansion.** The lazy build got 7–9 % faster; the mechanism was
  not isolated. The lazy join got 4–5 % slower. The one part of its path
  that changed is expansion: each child it reaches is now built by
  group-then-build instead of by re-inserting its few tuples.
- **`child-capacity=tuples` under `bulk`.** Sized children skip their
  rehashes. Under `presized:16` it changes nothing measurable; why was not
  investigated.
- **Expected effects that did not show.** The `space` rise predicted under
  Config flags (|L| > D) did not appear: these binary relations are sets, so
  a depth-1 list holds one tuple per distinct key. The predicted fall did
  appear, on `price`. Its one-tuple children get 2 buckets instead of 4,
  which saves 18 % of the space and cuts its iteration time by 16 %.

### Chain links, measured (#101 B2)

B2 is not an axis: every `HashTrie` threads its lists through `links`. Its
records therefore compare the commit before B2 with a commit after it. Both
ran on an AMD Ryzen 7 7700X with jemalloc 5.3.1, on SipHash with pruning
off unless a cell says otherwise.

#### JOB peak memory (2026-10-17/18)

Run directory: `kermit-bench-runs/job-97/old-a589c64/` (`run.sh`,
`run2.sh`, `run.log`). The after-B2 runs are #97's J9 acceptance runs, in
`kermit-bench-runs/job-97/accept/`.
- BEFORE is a589c64, B1's landing and the commit before B2 (sha256
  `ce30fd93ba0399cd…`), built from a `git archive` in its own target
  directory.
- AFTER is b89f29c, the J9 acceptance binary (sha256 `1aab9777c3634b90…`).
- Between the two, B2 is the only engine change on the default `bulk` path.
  The toolchain, jemalloc and `parquet` (59.3.1) are the same.

**Method.**
- Each template ran once per binary: `bench run job-N -i hash-trie -a
  hash-triejoin --verify -m space`, with `--sample-size 10
  --measurement-time 1 --warm-up-time 1`, on the default Layout and build
  (eager, `bulk`).
- Each process ran in its own 14 GiB cgroup cap with no swap. Peak memory
  is GNU time's maximum RSS of the whole process.
- Two BEFORE runs (job-6 and job-17) were killed by host-wide OOMs, not by
  the cap. Each was re-run once the host had the memory, and only the
  re-runs count.

**Answers.** 32 of the 33 templates finished on both binaries, and all 109
of their query counts matched DuckDB's on both. job-8 exceeds the 14 GiB
cap on both.

**Peak memory.** BEFORE ÷ AFTER is 1.13–1.26 over the 32, geomean 1.18, so
the chains save about 15 % of HashTrie's peak. The ratio barely depends on
size: 1.15–1.26 on templates that peak under 6 GiB, 1.13–1.20 on the six
that peak at 8.9–10.9 GiB. AFTER's peaks for all three structures are in
[`JOB.md`](../benchmarks/JOB.md#practical-scale-ceiling).

| Template | AFTER (GiB) | BEFORE (GiB) | BEFORE ÷ AFTER |
|---|---|---|---|
| 1 | 0.90 | 1.13 | 1.26 |
| 2 | 1.86 | 2.23 | 1.20 |
| 3 | 1.50 | 1.85 | 1.23 |
| 4 | 1.81 | 2.17 | 1.20 |
| 5 | 0.93 | 1.11 | 1.20 |
| 6 | 9.36 | 11.27 | 1.20 |
| 7 | 8.95 | 10.65 | 1.19 |
| 8 | > 14 (cap) | > 14 (cap) | — |
| 9 | 2.37 | 2.84 | 1.20 |
| 10 | 1.74 | 2.06 | 1.19 |
| 11 | 2.59 | 2.99 | 1.16 |
| 12 | 4.87 | 5.69 | 1.17 |
| 13 | 5.15 | 6.04 | 1.17 |
| 14 | 2.31 | 2.69 | 1.16 |
| 15 | 2.70 | 3.15 | 1.17 |
| 16 | 10.86 | 12.96 | 1.19 |
| 17 | 10.71 | 12.78 | 1.19 |
| 18 | 4.93 | 5.90 | 1.20 |
| 19 | 5.55 | 6.55 | 1.18 |
| 20 | 9.16 | 10.38 | 1.13 |
| 21 | 2.32 | 2.75 | 1.19 |
| 22 | 3.77 | 4.35 | 1.16 |
| 23 | 2.63 | 3.04 | 1.15 |
| 24 | 2.84 | 3.34 | 1.18 |
| 25 | 2.24 | 2.68 | 1.19 |
| 26 | 9.55 | 10.78 | 1.13 |
| 27 | 2.31 | 2.72 | 1.17 |
| 28 | 2.35 | 2.74 | 1.17 |
| 29 | 4.18 | 4.87 | 1.16 |
| 30 | 2.41 | 2.88 | 1.20 |
| 31 | 3.26 | 3.92 | 1.20 |
| 32 | 1.25 | 1.51 | 1.21 |
| 33 | 2.53 | 2.96 | 1.17 |

**Time.** These were not timing runs. 78–84 % of each run's wall time is
Criterion's space sampling, which lasts a fixed ~2 s per relation per query
whatever the binary does (1.01× between the binaries). The rest of the run
reads the Parquet files, builds every relation and runs one verify join per
query. That part took BEFORE 1.47× as long per template (geomean, range
0.93–2.26). Each template ran once on a shared host, so this shows a
direction only. It agrees with the build ratios below.

#### Replicated B1 + B2 run, interim (2026-10-14/15)

Run directory: `kermit-bench-runs/hash-trie-chain-links-2026-10-14/`
(README, `run.sh`, `analyse.py`); these numbers are
`logs/interim-1122.txt`. The design's § Measurement planned 5 replicates.
The run was cut to 3, paused during replicate 3 and then dropped
(2026-10-16), so these are interim numbers over 2–3 replicates (n = 1
where marked).

B2 = d2bfc43 ÷ a589c64, so below 1 B2 is faster or smaller. TreeTrie's code
is identical in both binaries, and its `bench ds iteration` ratio of
0.93–0.95 is this run's codegen bound.

| Metric | B2 ratio |
|---|---|
| `bench ds insertion`, eager, per build mode (geomean over inputs) | 0.74–0.90 |
| triangle (`oxford-uniform-s3`) `insertion`, eager, pruning off / on | 0.67 / 0.74 |
| triangle `iteration` (the join), eager, off / on | 1.02 / 1.01 |
| triangle `space`, eager, off / on | 0.81 / 0.88 |
| `bench ds iteration` (the trie walk), eager, per build mode | 0.75–0.84 |
| `bench ds iteration`, lazy `bulk`, shuffled `friendof` | 11.2 [10.3, 15.7] |
| the same on `binary-1e7` in file order (n = 1) | 3.9 |
| the same on grouped `friendof` / sorted `binary-1e7` | 1.18 / 0.98 |

Part D, the check of what the atomics cost eager tries, is interim too. One
of its 26 cells fell outside the codegen bound, marginally: pruned
`incremental` `iteration` on `friendof`, plain links ÷ atomic links 0.969
[0.954, 0.984], so the plain-links walk was 3 % faster.

**Reading.**
- **Memory.** A list was a `Vec<RowId>`, a heap allocation of its own, and
  a leaf bucket was 32 B. A list is now a chain through 4 bytes of link per
  row, and a leaf bucket is 24 B. JOB's relations hold mostly short lists,
  so the saving is a near-constant share of the trie.
- **Build.** No build allocates or frees a list any more, the cost the
  [Algorithm 2 A/B](#algorithm-2-ab-107)'s *All keys distinct* reading
  named. The faster eager `insertion` is consistent with that; the
  mechanism was not isolated.
- **Join.** The eager triangle join is unchanged. The eager trie walk is
  faster, part of it codegen, since TreeTrie's walk moves by 0.93–0.95.
- **Lazy walk.** An unexpanded child's walk read a contiguous `Vec<RowId>`
  before B2, whose loads are independent. It now follows `links`, one
  dependent load per row, scattered over the buffer unless the input is
  grouped. This is unresolved. The JOB runs above are eager, so it does not
  touch them.
- **job-8** still exceeds 14 GiB after B2, so the chains are not enough for
  it.

## See also

- Sibling docs: [`TreeTrie`](./tree-trie.md), [`ColumnTrie`](./column-trie.md).
- [`HashTriejoin`](../algorithms/hash-triejoin.md) — the only algorithm that consumes this structure.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; every Layout combination of `HashTrie` (`HashTrieSip`, `HashTrieFx`, `HashTrieSipPruned`, `HashTrieFxPruned`, and each again with a `Lazy` suffix) must pass all 16 patterns under `HashTriejoin`, with both optimisers (Priorities item 1).
- `hash_trie_test_suite!` and `parquet_test_suite!` ([`kermit-ds/tests/common/macros.rs`](../../kermit-ds/tests/common/macros.rs)) — the layer below the join: `HashTrieIterator` contract (`open`/`next`/`lookup`/`up`/`size`/`leaf_tuples`), construction round-trips compared through a trie walk (`for_each_tuple`, since `collect_tuples()` only copies back the buffer the trie was built from), and Parquet loading, whose collector is `collect_tuples()`. `HashTrie` cannot use `relation_trie_test_suite!` (it is `HashTrieIterable`, not `TrieIterable`), so this hash-family suite mirrors it; each Layout alias runs it — `HashTrieSip`, `HashTrieFx` and the colliding `HashTrieMod10`, and each again with pruning on (`HashTrieSipPruned`, `HashTrieFxPruned`, `HashTrieMod10Pruned`) so the iterator contract holds on emulated levels too, and all six again with lazy expansion (`…Lazy`) so it holds while `open` expands children mid-iteration — plus `HashTrieSipDense = Configured<HashTrieSip, NinetyPercent>` and `HashTrieMod10Dense` for the Config axis. `HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>` runs the same suite with the root presized (`root-capacity=tuples`). At the join layer, `define_multiway_join_test_suite_with_config!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) runs the same 16 patterns under the `HalfFull` load-factor provider and the `PresizedRoot` root-capacity provider.
