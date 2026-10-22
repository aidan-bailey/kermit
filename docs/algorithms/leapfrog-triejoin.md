# `LeapfrogTriejoin`

> **Status:** stable · **CLI:** `-a leapfrog-triejoin` · **Implementation:** [`kermit_algos::leapfrog_triejoin`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs)

## What it is

Leapfrog Triejoin (LFTJ) is the worst-case-optimal multi-way join from [Veldhuizen 2014 (arXiv:1210.0481)](https://arxiv.org/abs/1210.0481). It coordinates one [`TrieIterator`](../data-structures/) per body predicate, descending the relations in lockstep variable-by-variable. At each depth it delegates the inner intersection to [`LeapfrogJoin`](./leapfrog-join.md); iterators are swapped in and out of the inner join as the depth changes so only the relations mentioning the current variable participate.

LFTJ's running time is bounded by the AGM bound — the theoretical worst-case output size — independent of input sizes. Any data structure satisfying the [`TrieIterable`](../../kermit-iters/src/) contract inherits this guarantee.

## Pseudocode

Variables `v_1, …, v_n` are joined in the order given by the `QueryPlan`'s `variable_ordering` — produced by a [`QueryOptimiser`](../optimisers/) before `join_for_each` runs, not computed by the algorithm itself. At depth `d` the algorithm uses [`LeapfrogJoin`](./leapfrog-join.md) over only the iterators whose relation mentions `v_d`.

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

Source mapping: [`triejoin_open`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs) (line 259), [`triejoin_up`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs) (line 298), [`update_iters`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs) (line 194), plan validation and variable ordering in [`join_for_each`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs) (line 357).

## State machine

Every body-predicate iterator is in **exactly one** of two places at any moment:

- **idle** — owned by `idle_iterators[i]` as `Some(iter)`, not in the active leapfrog;
- **active** — owned by `leapfrog.iterators`, participating at the current depth.

`active_iter_indices` is parallel to `leapfrog.iterators` (in pop order) and remembers which idle slot lent each active iterator. The *only* code that moves iterators across the idle/active boundary is `update_iters`, called by every depth change. All other code reads — never moves — iterators across this boundary.

`update_iters` refills the one inner `LeapfrogJoinIter` in place (`LeapfrogJoinIter::refill`) rather than building a new one, so the leapfrog's iterator and permutation buffers grow to the widest depth once and a depth change allocates nothing — before #83 it cost two allocations and two frees. `kermit/tests/result_allocation.rs` pins this: the allocation count must not grow with the number of descents.

## Invariants

- **Iterator depth tracks triejoin depth.** Every iterator in `leapfrog.iterators` has had `open()` called the same number of times as the triejoin's `depth`. `up()` on an active iterator must succeed; if it returns `false` the triejoin asserts and panics ([leapfrog_triejoin.rs:303](../../kermit-algos/src/sorted/leapfrog_triejoin.rs)) — this is always a programming error, not a recoverable runtime condition.
- **A failed descent is atomic.** When `triejoin_open` returns `false` — a participating iterator refused to `open`, or the leapfrog found no common key — it ascends the iterators that did descend, restores the parent depth and rebuilds the parent leapfrog before returning. This is what keeps the invariant above true on the pruning path, and it honours `TrieIterator::open`'s contract that a failed open leaves the position unchanged. `TrieIteratorWrapper` pushes nothing when `open` fails, so without the restore the wrapper's tuple stack runs one level behind the join and `next_sibling` overwrites the wrong key — silently dropping answers whenever the failure happens at depth 3 or deeper (it self-heals at shallower depths, because popping the emptied stack is a no-op). Regression: `valid_orderings_agree_when_a_deep_open_fails`.
- **`open()` is called even after `at_end()` returns true.** This is **not** a bug. LFTJ uses the `at_end` → `open` sequence to descend past a stale stack top, relying on the index structure's `open()` to push a *child of the current node* rather than positioning on the sibling. Index structures with a different `open` semantics will silently break LFTJ. See the LFTJ gotcha in `CLAUDE.md`.
- **Constants and repeated variables never reach the join.** `kermit::db::prepare` turns each body atom into an access (`kermit_algos::extract_accesses`, [access.rs](../../kermit-algos/src/access.rs)): a constant `c<id>` becomes a constant filter on its column, and the second and later occurrence of a variable inside one atom becomes an equality with its first. The build applies that filter while it copies the rows (`TupleView`, [tuple_view.rs](../../kermit-iters/src/tuple_view.rs)), so the join receives plain indexes, one per distinct (relation, filter, column order), whose columns are all distinct variables. LFTJ keeps the literature's one-variable-per-level assumption and no iterator wrapper stands between it and a trie. Before per-query index builds (2026-10-22), a one-value `Const_c<id>` relation and a `Select_<n>_<base>` equality view (a non-materialised `σ_{col_i = col_j}`) were wrapped around the iterators, and the view's refusal to `open` on a non-diagonal prefix was an ordinary dead prefix, handled by "A failed descent is atomic". An atom of constants only builds no index: its row count multiplies the results (`Indexes::multiplicity`), and a zero count ends the query. Regression: the `diagonal` and `repeated_nonadjacent` patterns of the standard suite, and `kermit/tests/all_orders.rs`. A new algorithm tolerates any index, since the executed query holds only `#<id>` atoms over distinct variables.
- **Placeholders never reach the join.** The entry point runs `rewrite_placeholders` ([placeholder_rewrite.rs](../../kermit-algos/src/placeholder_rewrite.rs)) first, so every `_` arrives as its own fresh variable and each atom carries one variable per column — the iterator opens one physical level per variable, so a skipped `_` used to shift the later columns onto the wrong level (`r(X, _, Y)` bound `Y` to the second column; issue #73). Regression: the `trailing_placeholder` and `middle_placeholder` patterns of the standard suite.
- **Valid global attribute order (GAO).** The descent order must bind every index's variables in physical column order, because each iterator descends one stored column per `open()`. The ordering arrives as a `QueryPlan` produced by a [`QueryOptimiser`](../optimisers/) (previously it was computed in-algorithm by a now-deleted `build_variable_index`/`global_attribute_order` pair); `join_for_each` validates it with [`QueryPlan::validate`](../../kermit-algos/src/optimiser/plan.rs) and panics on an invalid plan, then permutes each result tuple back to head-first order into one scratch row (lent by `TrieIteratorWrapper::advance`, so no tuple is allocated) so output columns are independent of the descent order. The default [`LexicographicOptimiser`](../optimisers/lexicographic.md) binds the variables in canonical order. The plan can always be satisfied, because the indexes follow the plan: the planner is free, and [`plan_indexes`](../../kermit-algos/src/index_plan.rs) sorts each access's kept columns by the plan position of their variables, so every atom LFTJ sees is an index whose column order is the order the plan binds its variables in. A cyclic pair such as `r(X, Y), s(Y, X)` plans like any other query, with one of the two read through an index whose columns are swapped. A constant is no longer a variable that must be bound in a particular place: `p(c, X)` is a filter on `p`'s first column, applied when `p`'s index is built, and `kermit/tests/subject_position_constant.rs` and the LUBM cardinality test still guard it end to end. Before per-query index builds (2026-10-22), each relation's stored column order constrained the plan, a cyclic constraint set was rejected with `JoinError::CyclicAttributeOrder`, and a naive first-appearance order silently yielded 0 results for subject-position constants. The hash-trie join shares the same `QueryPlan` contract.

## Complexity

The output-sensitive cost is bounded by `AGM(Q)`, the AGM bound of the query — independent of the input sizes. Per-tuple cost is `O(depth · k · seek)` where `k` is the maximum number of iterators active at any single depth.

| Operation | Time | Notes |
|---|---|---|
| `triejoin_open` | O(k · open + leapfrog_init) | descend one level |
| `triejoin_up` | O(k · up) | ascend one level |
| enumerating all tuples (`join_for_each`) | O(AGM(Q) · per-tuple work) | worst-case-optimal |

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

Result tuple: `(7, 4, 5)`. This is the test [`triangle_join_collect`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L600).

## See also

- [`LeapfrogJoin`](./leapfrog-join.md) — the inner k-way intersection used at every depth.
- [`TreeTrie`](../data-structures/tree-trie.md), [`ColumnTrie`](../data-structures/column-trie.md) — index structures that satisfy the `TrieIterable` contract.
- [`docs/optimisers/`](../optimisers/) — the `QueryOptimiser` implementations that plan the `QueryPlan` this algorithm executes.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — the 16 standard patterns LFTJ is tested against under every index structure (Priorities item 1).
