# `LeapfrogTriejoin`

> **Status:** stable · **CLI:** `-a leapfrog-triejoin` · **Implementation:** [`kermit_algos::leapfrog_triejoin`](../../kermit-algos/src/leapfrog_triejoin.rs)

## What it is

Leapfrog Triejoin (LFTJ) is the worst-case-optimal multi-way join from [Veldhuizen 2014 (arXiv:1210.0481)](https://arxiv.org/abs/1210.0481). It coordinates one [`TrieIterator`](../data-structures/) per body predicate, descending the relations in lockstep variable-by-variable. At each depth it delegates the inner intersection to [`LeapfrogJoin`](./leapfrog-join.md); iterators are swapped in and out of the inner join as the depth changes so only the relations mentioning the current variable participate.

LFTJ's running time is bounded by the AGM bound — the theoretical worst-case output size — independent of input sizes. Any data structure satisfying the [`TrieIterable`](../../kermit-iters/src/) contract inherits this guarantee.

## Pseudocode

Variables `v_1, …, v_n` are joined in the order given by the `QueryPlan`'s `variable_ordering` — produced by a [`QueryOptimiser`](../optimisers/) before `join_iter` runs, not computed by the algorithm itself. At depth `d` the algorithm uses [`LeapfrogJoin`](./leapfrog-join.md) over only the iterators whose relation mentions `v_d`.

```
triejoin_open():       # descend
    depth ← depth + 1
    refresh active iterators for new depth    # update_iters
    for each active iter:  iter.open()
    leapfrog.init()

triejoin_up():         # ascend
    for each active iter:  iter.up()          # panics if up fails (LFTJ invariant)
    depth ← depth - 1
    refresh active iterators for new depth    # update_iters

# Tuple enumeration is driven externally via IntoIterator over
# TrieIteratorWrapper, which walks the depth in DFS order.
```

Source mapping: [`triejoin_open`](../../kermit-algos/src/leapfrog_triejoin.rs) (line 220), [`triejoin_up`](../../kermit-algos/src/leapfrog_triejoin.rs) (line 247), [`update_iters`](../../kermit-algos/src/leapfrog_triejoin.rs) (line 180), plan validation and variable ordering in [`join_iter`](../../kermit-algos/src/leapfrog_triejoin.rs) (line 306).

## State machine

Every body-predicate iterator is in **exactly one** of two places at any moment:

- **idle** — owned by `idle_iterators[i]` as `Some(iter)`, not in the active leapfrog;
- **active** — owned by `leapfrog.iterators`, participating at the current depth.

`active_iter_indices` is parallel to `leapfrog.iterators` (in pop order) and remembers which idle slot lent each active iterator. The *only* code that moves iterators across the idle/active boundary is `update_iters`, called by every depth change. All other code reads — never moves — iterators across this boundary.

## Invariants

- **Iterator depth tracks triejoin depth.** Every iterator in `leapfrog.iterators` has had `open()` called the same number of times as the triejoin's `depth`. `up()` on an active iterator must succeed; if it returns `false` the triejoin asserts and panics ([leapfrog_triejoin.rs:254](../../kermit-algos/src/leapfrog_triejoin.rs)) — this is always a programming error, not a recoverable runtime condition.
- **`open()` is called even after `at_end()` returns true.** This is **not** a bug. LFTJ uses the `at_end` → `open` sequence to descend past a stale stack top, relying on the index structure's `open()` to push a *child of the current node* rather than positioning on the sibling. Index structures with a different `open` semantics will silently break LFTJ. See the LFTJ gotcha in `CLAUDE.md`.
- **Const rewrite tolerance.** `DatabaseEngine::join` rewrites every `Term::Atom("c<id>")` into a fresh variable plus a synthetic unary `Const_c<id>` predicate (see [const_rewrite.rs](../../kermit-algos/src/const_rewrite.rs) and the const-view-rewrite gotcha). LFTJ never sees atoms; new algorithms must also tolerate the rewritten form.
- **Valid global attribute order (GAO).** The descent order must bind every relation's variables in physical column order, because each iterator descends one stored column per `open()`. The ordering arrives as a `QueryPlan` produced by a [`QueryOptimiser`](../optimisers/) (previously it was computed in-algorithm by a now-deleted `build_variable_index`/`global_attribute_order` pair); `join_iter` validates it with [`QueryPlan::validate`](../../kermit-algos/src/optimiser/plan.rs) and panics on an invalid plan, then permutes each result tuple back to head-first order so output columns are independent of the descent order. The default [`LexicographicOptimiser`](../optimisers/lexicographic.md) reproduces the previously hardcoded Kahn's-with-smallest-index order, so documented behaviour is unchanged by default. A naive first-appearance order silently broke subject-position constants — `p(c, X)` rewrites to `p(K, X), Const(K)` where `K` is physically first but appears last — yielding 0 results; this is guarded by `kermit/tests/subject_position_constant.rs` and the LUBM cardinality test. A cyclic constraint set (e.g. `r(X, Y), s(Y, X)`) cannot be answered with a single trie order per relation and panics (in [`topological_order`](../../kermit-algos/src/optimiser/ordering.rs), called from the optimiser's `plan()`, not from `join_iter`). The hash-trie join shares the same `QueryPlan` contract.

## Complexity

The output-sensitive cost is bounded by `AGM(Q)`, the AGM bound of the query — independent of the input sizes. Per-tuple cost is `O(depth · k · seek)` where `k` is the maximum number of iterators active at any single depth.

| Operation | Time | Notes |
|---|---|---|
| `triejoin_open` | O(k · open + leapfrog_init) | descend one level |
| `triejoin_up` | O(k · up) | ascend one level |
| collecting all tuples | O(AGM(Q) · per-tuple work) | worst-case-optimal |

## Worked micro-example

Triangle join `Q(a, b, c) :- R(a, b), S(b, c), T(a, c)` with:

```
R = {(7, 4)}
S = {(4, 1), (4, 4), (4, 5), (4, 9)}
T = {(7, 2), (7, 3), (7, 5)}
```

Variable ordering `[a, b, c]`:

1. **Depth 1 (`a`).** `R` and `T` carry `a`, so they participate. Intersect on `a`: `R` exposes `7`, `T` exposes `7` → common = `{7}`.
2. **Depth 2 (`b`).** `R` and `S` carry `b`. With `a = 7` fixed, `R(7, …)` exposes `b = 4`; `S` starts at `b = 4`. Intersection on `b`: `{4}`.
3. **Depth 3 (`c`).** `S` and `T` carry `c`. With `b = 4` fixed, `S(4, …) = {1, 4, 5, 9}`. With `a = 7` fixed, `T(7, …) = {2, 3, 5}`. Intersection: `{5}`.

Result tuple: `(7, 4, 5)`. This is the test [`triangle_join_collect`](../../kermit-algos/src/leapfrog_triejoin.rs#L671).

## See also

- [`LeapfrogJoin`](./leapfrog-join.md) — the inner k-way intersection used at every depth.
- [`TreeTrie`](../data-structures/tree-trie.md), [`ColumnTrie`](../data-structures/column-trie.md) — index structures that satisfy the `TrieIterable` contract.
- [`docs/optimisers/`](../optimisers/) — the `QueryOptimiser` implementations that plan the `QueryPlan` this algorithm executes.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — the 11 standard patterns LFTJ is tested against under every index structure (Priorities item 1).
