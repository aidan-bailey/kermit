# `HashIndex`

> **Status:** experimental · **CLI:** `-i hash-index` (pairs only with `-a binary-hash-join`) · **Implementation:** [`kermit_ds::ds::hash_index`](../../kermit-ds/src/ds/hash_index/)

## Representation

`HashIndex<H: HashStrategy = SipHashStrategy>` is the textbook chained hash table of a binary hash join: Free Join §2.2's "hash table keyed on" the attributes a relation shares with the relations before it in a left-deep plan (Wang et al., SIGMOD 2023). It is not a trie: it has one composite level, the key, and the rest of each row is payload kept in the row.

```rust
HashIndex<H: HashStrategy = SipHashStrategy> {
    header: RelationHeader,
    rows: Tuples,            // view.materialize(): one copy, in source order
    key_len: usize,          // header.levels()
    directory: Vec<RowId>,   // `slots` bucket heads, NIL when empty; empty when key_len == 0
    next: Vec<RowId>,        // per row, the next row of its bucket's chain; empty when key_len == 0
    _hasher: PhantomData<H>, // hasher, zero-sized
}
```

`NIL` is `RowId::MAX`, which is never a row id (a `Tuples` batch holds at most `RowId::MAX` rows). `directory` and `next` are plain `Vec<RowId>`, not `ChainLinks`: `ChainLinks` exists so that a lazy `HashTrie` can re-thread a child's rows through `&self`, and a keyed index never re-threads anything.

- **Sizing.** `slots = max(2, rows.next_power_of_two())`, so the load factor is at most 1 and the bucket shift (`64 - log2(slots)`) never reaches 64.
- **Hashing.** The key hash folds `H::hash` over the key columns with `FxHash`'s combine step, `h = (h.rotate_left(5) ^ H::hash(v)).wrapping_mul(0x517c_c1b7_2722_0a95)`, from `h = 0`. The bucket is the top `log2(slots)` bits of `h.wrapping_mul(0x9E37_79B9_7F4A_7C15)` (Fibonacci hashing). `HashTrie` uses the same multiply-then-take-top-bits idea in its own code; neither structure depends on the other's.
- **Probe.** `probe(key)` hashes `key`, walks the bucket's chain, and lends each row whose first `key_len` values equal `key`; rows of other keys in the same bucket are skipped by that comparison. With `key_len == 0` it lends every row.

### Construction

`from_view(header, view)` reads a [`TupleView`](../../kermit-iters/src/tuple_view.rs) and copies the matching rows once, with `view.materialize()`: an exact-size buffer, in source order, which becomes the index's own `rows`. It then allocates `directory` and `next` and links every row from last to first, each prepended to its bucket's chain, so every chain ends in source order. That is three allocations (`rows`, `directory`, `next`) and none per input row. The build only reads the source, the catalog's flat area. There is one build and no BuildMode: `bench ds` and `bench run` time the same `from_view`. `insert` appends a row and links it at its chain's tail, and doubles the directory with a rehash once rows outnumber slots; it is for tests and `Relation::new` users and is not benchmarked.

The key length is `header.levels()`. `kermit::db::prepare` plans an index under `TrieLevels::ProbeKeys`, which sets each spec's `levels` to the length of the key its atom is probed on (`0` for the join order's first atom). `bench ds` builds from a header with no query, whose `levels()` is the arity, so the index is keyed on every column.

## Invariants

- **Chains are in source order.** Linking last to first prepends each row to its bucket, so each chain lists its rows in ascending row id; `probe` and `scan` therefore lend rows in source order. `insert` keeps this by appending to a chain's tail and relinking last to first on a rehash, so an index built by inserts has the arrays a bulk build of the same rows has (`insert_builds_the_arrays_the_bulk_build_builds`).
- **`key_len == header.levels()`**, at most the arity. `probe` asserts `key.len() == key_len()`: a key of the wrong length would compare the wrong columns.
- **The directory is a power of two, at least 2, and at least the row count**, and `directory` and `next` are empty exactly when `key_len == 0`.
- **Duplicates are kept.** `Relation::KEEPS_DUPLICATES` is `true`: a repeated row is stored and lent once per copy, `Cardinality::tuple_count` counts it, and an all-constant atom's row count multiplies a join's results, as on `HashTrie` (`kermit::db::build` clamps it only for structures that absorb duplicates).
- **No collision means no match.** A hash collision only puts rows of other keys in the same chain. The key comparison lends a row only if its key columns equal the probe's key, so `probe` is exact whatever the hasher does (`keyed_index_test_suite!` runs it under a hasher that sends every value to bucket 0).

## Complexity

Let `n` = tuple count, `a` = arity, `k` = `key_len`, and `c` = the length of the chain a probe walks (about 1 in expectation, `n` for a hasher that sends every key to one bucket).

| Operation | Time | Space | Notes |
|---|---|---|---|
| `insert(tuple)` | expected O(a + c), O(n) at a doubling | O(a) | appends to the chain's tail; a rehash relinks all `n` rows |
| `from_view(view)` | O(n · (a + k)) | O(n · a + n) | `view.materialize()` (a filtered view also costs O(s) filter tests over the source's s rows), then one hash of `k` values per row |
| `KeyedIterable::key_len()` | O(1) | | field read |
| `KeyedIterable::scan()` | O(1) to start, O(1) per row | | walks `rows` |
| `KeyedIterable::probe(key)` | expected O(k + c · k) | | one hash of `k` values, then up to `c` key comparisons of `k` values; `O(1)` to start when `k == 0` |
| `HeapSize::heap_size_bytes()` | O(1) | | `rows` + `directory` + `next`, by capacity |

Space is `8 · a · n` bytes of rows, `4 · slots` bytes of directory (at most `8 · n` once `n ≥ 1`, since `slots ≤ 2n`) and `4 · n` bytes of links: at most `8 · a · n + 12 · n` bytes.

## Worked micro-example

The triangle `Q(A, B, C) :- R(A, B), S(B, C), T(A, C)` under the variable order `[A, B, C]` has the join order `R, S, T` (Free Join Example 2.2): `R` is scanned, `S` is probed on `B`, `T` on `(A, C)`. So `S` is an index of `key_len == 1` and `T` of `key_len == 2`. The illustration takes the identity hash `h(v) = v`, which is not one of kermit's hashers (they scatter the keys); the structure is the same.

`S(B, C) = {(2, 3), (2, 4), (3, 4)}`, keyed on `B`. Three rows, so 4 slots (the top two bits of the Fibonacci product): key `(2)` falls in bucket 0 and key `(3)` in bucket 2.

```
rows        0:(2,3)   1:(2,4)   2:(3,4)
directory   [0]=row 0  [1]=NIL  [2]=row 2  [3]=NIL
next        row 0 → row 1   row 1 → NIL   row 2 → NIL
```

- `probe(&[2])`: bucket 0, row 0 `(2, 3)` key matches, row 1 `(2, 4)` key matches, end. Both rows lent, in source order.
- `probe(&[3])`: bucket 2, row 2 `(3, 4)`. One row.

`T(A, C) = {(1, 3), (1, 4)}`, keyed on both columns. Two rows, so 2 slots (the top bit): keys `(1, 3)` and `(1, 4)` both fall in bucket 1, a collision.

```
rows        0:(1,3)   1:(1,4)
directory   [0]=NIL    [1]=row 0
next        row 0 → row 1   row 1 → NIL
```

- `probe(&[1, 4])`: bucket 1, row 0 `(1, 3)` is skipped (its key differs), row 1 `(1, 4)` is lent.
- `probe(&[1, 5])`: bucket 0 is empty, nothing is lent.

Take `R = {(1, 2), (1, 3)}`. For `(A, B) = (1, 2)`, `S.probe(&[2])` lends `(2, 3)` and `(2, 4)`; `T.probe(&[1, 3])` and `T.probe(&[1, 4])` each lend one row, so `(1, 2, 3)` and `(1, 2, 4)` are emitted. For `(1, 3)`, `S.probe(&[3])` lends `(3, 4)` and `T.probe(&[1, 4])` lends `(1, 4)`: `(1, 3, 4)`. The work is the sum of the intermediate results, `|R ⋈ S|` probes into `T`; see [`binary-hash-join.md`](../algorithms/binary-hash-join.md).

## When to prefer this structure

It is the baseline's structure, not a competitor to the tries: it exists to put a number on what worst-case optimal joins buy over a conventional binary plan. Use it with `-a binary-hash-join`, the only algorithm that drives it, and compare it with `HashTrie` under the same `--ds-layout-hasher`. On acyclic queries with selective joins it is a sound plan; on cyclic queries (a triangle, a 4-cycle) its intermediate results can exceed the AGM bound by orders of magnitude, which is the gap the baseline measures.

## Optimizations

| Dimension | Category | Axis | Flag | Default | Test aliases |
|---|---|---|---|---|---|
| Hasher | Layout (`H: HashStrategy`) | `ds_layout_hasher` | `--ds-layout-hasher sip\|fxhash` | `sip` | `HashIndexSip`, `HashIndexFx` |

The hasher changes only the key hash: the sizing, the build and the answers are the same under every `H`. It reuses `HashTrie`'s flag and its two hashers, so a HashTrie-against-HashIndex comparison can hold the hasher fixed. There are no Config and no BuildMode axes: the baseline has one build with a fixed sizing rule, so `--ds-config` and `--ds-build` are refused for `hash-index`.

## See also

- [`HashTrie`](./hash-trie.md) — the hash-based trie; the same hashers, a trie's levels instead of one composite key.
- [`BinaryHashJoin`](../algorithms/binary-hash-join.md) — the algorithm that drives this structure.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; `HashIndex` must pass all 16 patterns under `BinaryHashJoin` and every optimiser (Priorities item 1).
- [`kermit-ds/tests/hash_index_tests.rs`](../../kermit-ds/tests/hash_index_tests.rs) — the keyed contract suite, run under both hashers and a one-bucket strategy.
