# `LeapfrogJoin`

> **Status:** stable · **CLI:** internal helper — not CLI-exposed (used by [`LeapfrogTriejoin`](./leapfrog-triejoin.md)) · **Implementation:** [`kermit_algos::leapfrog_join`](../../kermit-algos/src/leapfrog_join.rs)

## What it is

`LeapfrogJoin` performs the **k-way sorted intersection** that sits at the heart of [`LeapfrogTriejoin`](./leapfrog-triejoin.md). Given `k` sorted [`LinearIterator`](../../kermit-iters/src/)s over `usize` keys, it emits the keys that appear in all `k` of them, in sorted order. Worst-case running time is `O(k · M · log(N/M))` where `M` is the smallest input and `N` the largest — output-sensitive in the smallest set.

The implementation follows the original Leapfrog Triejoin paper, [Veldhuizen 2014 (arXiv:1210.0481)](https://arxiv.org/abs/1210.0481), §3.

## Pseudocode

The k iterators are arranged in a logical ring sorted by their current key. The ring head's predecessor holds the *largest* key any iterator currently holds; the algorithm seeks the next iterator forward to that key, then rotates the ring.

```
init():
    if any iterator is at_end:  return false
    sort iterators by current key into a ring permutation
    p ← 0
    return search()

search():
    target ← key of iterator at ring[p - 1]   # the largest current key
    loop:
        current ← key of iterator at ring[p]
        if current == target:  return true    # common key found
        seek iterator at ring[p] to target
        if at_end:  return false
        target ← new key of iterator at ring[p]
        p ← (p + 1) mod k

next():
    advance iterator at ring[p]
    if at_end:  return None
    p ← (p + 1) mod k
    return search()
```

Source mapping: [`leapfrog_init`](../../kermit-algos/src/leapfrog_join.rs) (line 89), [`leapfrog_search`](../../kermit-algos/src/leapfrog_join.rs) (line 111), [`leapfrog_next`](../../kermit-algos/src/leapfrog_join.rs) (line 146).

## Invariants

- **Ring order.** `sorted_iter_perm` is a permutation of `0..k` ordering iterators by their *initial* current key (set in `leapfrog_init`). The ring is not re-sorted on each step — the monotone-increasing target ensures whichever iterator was largest remains the predecessor that defines `target_key`.
- **Monotone target.** Each iteration of `leapfrog_search` only ever raises `target_key`. Each `seek` either (a) reaches equality and returns `true`, (b) runs off the end and returns `false`, or (c) advances the current iterator strictly past the old target. Because keys are sorted ascending, the target rises only finitely many times before some iterator exhausts.
- **Underlying iterator contract.** Every input `LinearIterator` must be sorted ascending; `seek(k)` is forward-only and assumes `k ≥ self.key()`. Violations panic in the underlying iterator (e.g. [`TreeTrieIter::seek`](../data-structures/tree-trie.md)).

## Complexity

| Operation | Time | Notes |
|---|---|---|
| `leapfrog_init` | O(k log k + k · seek) | one sort + up to one seek per iterator |
| `leapfrog_search` | amortised O(seek) per ring step | total advances bounded by input sizes (monotone target) |
| `leapfrog_next` | O(seek) amortised | one underlying `next` + one search step |
| `leapfrog_seek(t)` | O(seek) | jump current ring head to `t`, then re-search |

Global bound: a full enumeration costs `O(k · M · log(N/M))` where `M` is the smallest input and `N` the largest.

## Worked micro-example

Three sorted vectors:

```
A = [1, 2, 3, 5]
B = [2, 4, 5, 6]
C = [2, 5, 7]
```

`leapfrog_init` reads each iterator's first key (`1, 2, 2`), sorts them into the ring `[A, B, C]` (ties don't matter), sets `p = 0`.

`leapfrog_search` from `p = 0`:

1. predecessor = `C`, `target = key(C) = 2`. `current = key(A) = 1 ≠ target`.
2. Seek `A` to `2`. `A` advances to `2`. `target = 2`. `p = 1`.
3. `current = key(B) = 2 == target` → return `true`.

First common key = `2`. `leapfrog_next` advances the ring head, eventually surfacing the next common key `5`, then exhausts. See test [`test_leapfrog_join_iter_multiple_vectors_with_common_elements`](../../kermit-algos/src/leapfrog_join.rs#L225).

## See also

- [`LeapfrogTriejoin`](./leapfrog-triejoin.md) — multi-way trie join that invokes `LeapfrogJoin` at every depth.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — `LeapfrogTriejoin` (which wraps `LeapfrogJoin`) must pass all 14 patterns under every index structure (Priorities item 1).
