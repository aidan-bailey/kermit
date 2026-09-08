# `HashTriejoin`

> **Status:** experimental · **CLI:** `-a hash-triejoin` · **Implementation:** [`kermit_algos::hash_triejoin`](../../kermit-algos/src/hash_triejoin.rs)

## What it is

A worst-case-optimal multi-way join over hash-trie-structured relations. Implements Algorithm 3 from "Combining Worst-Case Optimal and Traditional Binary Join Processing" (Freitag et al., SIGMOD 2020) — the probe phase of the paper's hash trie join.

The algorithm has the same asymptotic guarantee as Leapfrog Triejoin (output size bounded by the AGM bound) but uses hash-based intersection (`lookup(h)`) instead of comparison-based intersection (least-upper-bound `seek`). This makes it competitive on workloads where the seek dominates LFTJ's cost — typically large fan-outs.

## Pseudocode

```
function enumerate(i):                              # Alg. 3 line 1
    if i ≤ n:                                       # line 2
        I_join  = {I_j ∈ I | v_i ∈ E_j}             # line 3
        I_other = {I_j ∈ I | v_i ∉ E_j}             # line 4
        I_scan  = argmin_{I_j ∈ I_join} size(I_j)   # line 5
        repeat                                       # line 6
            # Probe other tables
            for I_j ∈ I_join \ I_scan:              # line 7
                if not lookup(I_j, hash(I_scan)):   # line 8
                    skip current iteration          # line 9
            for I_j ∈ I_join: down(I_j)             # lines 10-11
            enumerate(i + 1)                        # line 12
            for I_j ∈ I_join: up(I_j)               # lines 13-14
        until not next(I_scan)                      # line 15
    else:                                            # line 16
        for t ∈ ⨯_{I_j ∈ I_join} tuples(I_j):       # line 17
            if join condition holds for t:           # line 18
                produce(t)                           # line 19
```

Rust mapping:

- `enumerate(i)` → `fn enumerate(i, arity, iters, predicate_variables, variable_to_iter_map, output)` in [`kermit-algos/src/hash_triejoin.rs`](../../kermit-algos/src/hash_triejoin.rs).
- Lines 16–19 → `fn emit_leaf(iters, predicate_variables, arity, output)`.
- Line 18's verification → `fn verify_and_construct(candidate, predicate_variables, arity)`.

## Invariants

- **Iterators in `I_join` are in lockstep.** When `enumerate(i)` calls `open()` on every iterator in `I_join`, all of them descend to depth `i+1`. The `up()` calls after recursion symmetrically restore the depth. Any iterator that fails `open()` short-circuits the descent — the corresponding `up()` calls are limited to the iterators that successfully opened, preventing depth drift.
- **`enumerate` opens iterators at entry and ascends at exit.** The design diverged from the paper's pseudocode here for ergonomics — Algorithm 3 expects callers to have already descended the iterators before invoking `enumerate(i)`. The Rust implementation works on un-opened iterators: it issues `open()` on every participant in `I_join` at the start of the call and matches each successful descent with an `up()` before returning. This keeps the caller (the top-level `join_iter` entry point) free of bookkeeping at the cost of one extra descend/ascend per recursive level.
- **`I_scan` choice is local to each level.** The argmin is recomputed at every depth; the choice doesn't affect correctness, only the cost of probing the other iterators.
- **Hash collision rejection is at the leaves.** Inner-level `lookup(h)` may succeed on a false-positive hash match. The leaf-level `verify_and_construct` is the only place where actual key equality is enforced — every result tuple emerges from there.
- **Singleton iterators participate normally.** The const-view rewrite produces synthetic unary predicates backed by `SingletonHashTrieIter`. These satisfy `HashTrieIterator`'s contract via the [`HashTrieIterKind::Singleton`](../../kermit-algos/src/hash_trie_iter_kind.rs) variant.
- **The result is materialised, not streamed.** `join_iter` runs `enumerate` to completion, collecting every tuple into a `Vec`, and only then returns `output.into_iter()`. This satisfies the same `impl Iterator` signature as [LFTJ](leapfrog-triejoin.md), which yields lazily, so the asymmetry is invisible to callers that consume the whole result (as `DB::join` does). Any future time-to-first-tuple or peak-memory metric would be comparing unlike things; see the note on `JoinAlgo::join_iter`.
- **Variable ordering arrives as a plan, not a self-computed order.** `join_iter` no longer derives the descent order itself (the earlier per-algorithm `build_variable_index`/`global_attribute_order` helper has been deleted); it receives a `QueryPlan` produced ahead of time by a [`QueryOptimiser`](../optimisers/), validates it with [`QueryPlan::validate`](../../kermit-algos/src/optimiser/plan.rs), and panics on an invalid plan. The default [`LexicographicOptimiser`](../optimisers/lexicographic.md) reproduces the previously hardcoded Kahn's-with-smallest-index order, so documented behaviour is unchanged by default. LFTJ shares the exact same `QueryPlan` contract.

## Complexity

Let `n_j = |R_j|`, `m = |body predicates|`, `x = fractional edge cover of the query hypergraph`. From the paper's Theorem 1 (specialised to bag semantics):

> Time complexity is in O(m · n · ∏ |H(I_j)|^x_j), space in O(m).

In terms of cost per call:

| Operation | Time | Notes |
|---|---|---|
| `enumerate(i)` per call | O(\|I_join\| · cost(lookup)) | sums over the loop body; `lookup` is O(1) expected |
| `emit_leaf` | O(∏ chain_size · arity) | cross-product over leaf chains, verify each candidate in O(arity) |
| `verify_and_construct` | O(arity + ∑ \|E_j\|) | bounded by the total number of predicate variables |

In the common case (no hash collisions, each chain has length 1), `emit_leaf` is O(arity) per result.

## Worked micro-example

Triangle query `Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).` with:

- R = {(1, 2)}
- S = {(2, 3), (2, 4)}
- T = {(1, 3), (1, 4)}

Variable ordering: `[X=0, Y=1, Z=2]`. `predicate_variables = [[0, 1], [1, 2], [0, 2]]`. `variable_to_iter_map = [[0, 2], [0, 1], [1, 2]]`.

Trace:

- `enumerate(0)`: variable X. `I_join = {0, 2}` (R and T). Pick `I_scan` = whichever of R, T has smaller root-table size (here both have 1 entry; assume R).
  - Probe T for `hash(X=1)`. Hit.
  - `open` both R and T → depth 2.
  - `enumerate(1)`: variable Y. `I_join = {0, 1}` (R and S). Pick `I_scan`.
    - Probe S for `hash(Y=2)`. Hit.
    - `open` R and S → depth 3.
    - `enumerate(2)`: variable Z. `I_join = {1, 2}` (S and T). Pick `I_scan`.
      - Probe T for `hash(Z=3)`. Hit. Descend.
      - `enumerate(3) = arity ≥ n+1`: `emit_leaf`.
        - Chains: R.leaf=[[1,2]], S.leaf=[[2,3]], T.leaf=[[1,3]]
        - Candidate (1,2)(2,3)(1,3): verify_and_construct → result vector [1, 2, 3]. **Emit.**
      - `up` S and T.
      - Probe T for `hash(Z=4)`. Hit. Descend.
      - emit_leaf: chains R.leaf=[[1,2]], S.leaf=[[2,4]], T.leaf=[[1,4]]. Candidate (1,2)(2,4)(1,4). verify yields [1, 2, 4]. **Emit.**
      - `up`. End of S's bucket scan.
    - `up` R and S.
  - End of R's bucket scan at depth 2. `up` R and T.
- End of R's bucket scan at depth 1.

Output (sorted): `[[1, 2, 3], [1, 2, 4]]`.

Related test: `join_algo_triangle` in [`kermit-algos/src/hash_triejoin.rs`](../../kermit-algos/src/hash_triejoin.rs) exercises this same triangle-query path under `HashTriejoin` (with different sample data).

## See also

- [`HashTrie`](../data-structures/hash-trie.md) — the only data structure this algorithm consumes.
- [`LeapfrogTriejoin`](./leapfrog-triejoin.md) — sibling worst-case-optimal algorithm using sorted (LFTJ) intersection.
- [`docs/optimisers/`](../optimisers/) — the `QueryOptimiser` implementations that plan the `QueryPlan` this algorithm executes.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; this algorithm must pass all 11 patterns under `HashTrie` (Priorities item 1).
