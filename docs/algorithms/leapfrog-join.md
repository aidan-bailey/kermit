# `LeapfrogJoin`

> **Status:** stable · **CLI:** internal helper — not CLI-exposed (used by [`LeapfrogTriejoin`](./leapfrog-triejoin.md)) · **Implementation:** [`LeapfrogJoinIter`](../../kermit-algos/src/sorted/leapfrog_join.rs) (crate-private)

## What it is

`LeapfrogJoin` performs the **k-way sorted intersection** that sits at the heart of [`LeapfrogTriejoin`](./leapfrog-triejoin.md). Given `k` sorted [`LinearIterator`](../../kermit-iters/src/)s over `usize` keys, it emits the keys that appear in all `k` of them, in sorted order. Worst-case running time is `O(k · M · log(N/M))` where `M` is the smallest input and `N` the largest — output-sensitive in the smallest set.

The implementation follows the original Leapfrog Triejoin paper, [Veldhuizen 2014 (arXiv:1210.0481)](https://arxiv.org/abs/1210.0481), §3.1: `leapfrog_init`, `leapfrog_search`, `leapfrog_next` and `leapfrog_seek` are its Algorithms 1–4.

## Pseudocode

The k iterators are arranged in a logical ring, kept sorted by current key read from the ring position `p`: the iterator at `p` holds the least key and its predecessor the greatest. The algorithm seeks the least iterator to the greatest key, then steps the ring on.

```
init():                                     # Algorithm 1
    if any iterator is at_end:  return false
    sort the ring by current key
    p ← 0
    return search()

search():                                   # Algorithm 2
    max ← key of iterator at ring[p - 1]    # the greatest current key
    loop:
        least ← key of iterator at ring[p]  # the least current key
        if least == max:  return true       # every iterator at the same key
        seek iterator at ring[p] to max
        if at_end:  return false
        max ← new key of iterator at ring[p]
        p ← (p + 1) mod k

next():                                     # Algorithm 3
    advance iterator at ring[p]
    if at_end:  return None
    p ← (p + 1) mod k
    return search() ? key : None

seek(t):                                    # Algorithm 4
    seek iterator at ring[p] to t
    if at_end:  return false
    p ← (p + 1) mod k
    return search()
```

Source mapping: [`leapfrog_init`](../../kermit-algos/src/sorted/leapfrog_join.rs#L85), [`leapfrog_search`](../../kermit-algos/src/sorted/leapfrog_join.rs#L110), [`leapfrog_next`](../../kermit-algos/src/sorted/leapfrog_join.rs#L131), [`leapfrog_seek`](../../kermit-algos/src/sorted/leapfrog_join.rs#L147).

## Invariants

- **Ring order.** `ring` is a permutation of `0..k`, and read cyclically from `p` it is sorted by current key. `leapfrog_init` establishes this by sorting. Each search step seeks the least iterator to at least the greatest key, making it the new greatest, and moves `p` on, which keeps the order; `next` and `seek` advance the iterator at `p` from a key every iterator shares, which keeps it too. The order is what lets `least == max` alone prove every iterator holds the same key.
- **Refill.** [`refill`](../../kermit-algos/src/sorted/leapfrog_join.rs) re-seeds a drained join over a new set of iterators and leaves it exactly as `new` would — the ring is the identity permutation of the new `0..k`, and `p = 0` — but in the existing buffers, so once they have grown to the widest set a refill allocates nothing. [`LeapfrogTriejoin`](./leapfrog-triejoin.md) refills its one inner join on every depth change (#83). After `triejoin_up` it steps the refilled join without re-running `leapfrog_init`; the identity ring is valid there because every iterator sits on the key it descended from, so any order is sorted.
- **Termination.** Each pass of `leapfrog_search` that does not return moves the iterator at `p` strictly forward: its least key differs from the greatest, and `seek` goes to the least key at or above it. So the loop ends within as many seeks as the iterators have keys, either with every key equal (`true`) or with one iterator off its end (`false`).
- **Underlying iterator contract.** Every input `LinearIterator` must be sorted ascending; `seek(k)` is forward-only and assumes `k ≥ self.key()`. Violations panic in the underlying iterator (e.g. [`TreeTrieIter::seek`](../data-structures/tree-trie.md)). A fresh `LinearIterator` sits before its first key, so `leapfrog_init` steps it there first; a trie iterator after `open` is already on one.

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

1. predecessor = `C`, `max = key(C) = 2`. `least = key(A) = 1 ≠ max`.
2. Seek `A` to `2`. `A` advances to `2`. `max = 2`. `p = 1`.
3. `least = key(B) = 2 == max` → return `true`.

First common key = `2`. `leapfrog_next` advances the ring head, eventually surfacing the next common key `5`, then exhausts. See test [`test_leapfrog_join_iter_multiple_vectors_with_common_elements`](../../kermit-algos/src/sorted/leapfrog_join.rs#L206).

## See also

- [`LeapfrogTriejoin`](./leapfrog-triejoin.md) — multi-way trie join that invokes `LeapfrogJoin` at every depth.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — `LeapfrogTriejoin` (which wraps `LeapfrogJoin`) must pass all 16 patterns under every index structure (Priorities item 1).
