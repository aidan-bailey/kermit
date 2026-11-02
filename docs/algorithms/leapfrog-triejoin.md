# `LeapfrogTriejoin`

> **Status:** stable · **CLI:** `-a leapfrog-triejoin` · **Implementation:** [`kermit_algos::LeapfrogTriejoin`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs)

## What it is

Leapfrog Triejoin (LFTJ) is the worst-case-optimal multi-way join from [Veldhuizen 2014 (arXiv:1210.0481)](https://arxiv.org/abs/1210.0481). It coordinates one [`TrieIterator`](../data-structures/) per body predicate, descending the relations in lockstep variable-by-variable. At each depth it delegates the inner intersection to [`LeapfrogJoin`](./leapfrog-join.md); iterators are swapped in and out of the inner join as the depth changes so only the relations mentioning the current variable participate.

LFTJ's running time is bounded by the AGM bound — the theoretical worst-case output size — independent of input sizes. Any data structure satisfying the [`TrieIterable`](../../kermit-iters/src/) contract inherits this guarantee.

## Pseudocode

Variables `v_1, …, v_n` are joined in the order given by the `QueryPlan`'s `variable_ordering` — produced by a [`QueryOptimiser`](../optimisers/) before `join_for_each` runs, not computed by the algorithm itself. At depth `d` the algorithm uses [`LeapfrogJoin`](./leapfrog-join.md) over only the iterators whose relation mentions `v_d`, its *participants*. The two trie operations are Veldhuizen's Algorithms 5 and 6; the linear-iterator operations (`key`, `next`, `seek`, `at_end`) are those of the current depth's leapfrog.

```
triejoin_open():                         # Algorithm 5: descend
    if depth = n:  return false
    depth ← depth + 1
    lend v_depth's participants to the leapfrog     # update_iters
    open each participant, stopping at the first that refuses
    if every one opened and leapfrog.init():  return true
    up() each one that opened            # a failed descent is atomic
    depth ← depth - 1
    lend v_depth's participants to the leapfrog     # update_iters
    return false

triejoin_up():                           # Algorithm 6: ascend
    if depth = 0:  return false
    up() each participant                # panics if up fails (LFTJ invariant)
    depth ← depth - 1
    lend v_depth's participants to the leapfrog     # update_iters
    return true
```

`TrieIteratorWrapper` walks the trie these present depth-first and lends each full-depth path; `join_for_each` scatters it into head-first order.

Source mapping: [`triejoin_open`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L189), [`triejoin_up`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L216), [`ascend`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L162) (the ascent both share), [`update_iters`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L134), plan validation and the scatter in [`join_for_each`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L278).

## State machine

Every body-predicate iterator is in **exactly one** of two places at any moment:

- **idle** — owned by `idle_iterators[i]` as `Some(iter)`, not in the active leapfrog;
- **active** — owned by `leapfrog.iterators`, participating at the current depth.

`active_iter_indices` is parallel to `leapfrog.iterators` and remembers which idle slot lent each active iterator. The *only* code that moves iterators across the idle/active boundary is `update_iters`, called by every depth change.

`update_iters` refills the one inner `LeapfrogJoinIter` in place (`LeapfrogJoinIter::refill`) rather than building a new one, so the leapfrog's iterator and permutation buffers grow to the widest depth once and a depth change allocates nothing — before #83 it cost two allocations and two frees. `kermit/tests/result_allocation.rs` pins this: the allocation count must not grow with the number of descents.

## Invariants

- **Iterator depth tracks triejoin depth.** Each iterator has been opened once for every depth up to the current one at which it participates. `up()` on an iterator that opened must succeed; if it returns `false` the triejoin panics ([`ascend`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L162)) — always a programming error, never a recoverable runtime condition.
- **A failed descent is atomic.** When `triejoin_open` returns `false` — a participant refused to `open`, or the leapfrog found no common key — it ascends the iterators that did descend, restores the parent depth and rebuilds the parent leapfrog before returning. This keeps the invariant above true on the pruning path and honours `TrieIterator::open`'s contract that a failed open leaves the position unchanged. `TrieIteratorWrapper` pushes nothing when `open` fails, so without the restore its tuple stack would run one level behind the join and `next_sibling` would overwrite the wrong key, silently dropping answers whenever the failure happens at depth 3 or deeper (at shallower depths it self-heals, because popping the emptied stack is a no-op). Regression: `valid_orderings_agree_when_a_deep_open_fails`.
- **`up()` is called on an exhausted iterator; `open()` never is.** The wrapper descends only from a depth whose leapfrog holds a common key, so every participant of that depth is on that key, and every other iterator is still on the key of its own last depth. The triejoin therefore never opens an iterator at its end, and an index structure need not support it. It does ascend from one: the iterator that ran off its end, exhausting a depth's leapfrog or failing its `init`, is among those `triejoin_up` or a failed descent moves up, so an index structure must support `up` after `at_end` (the note on `TrieIterator::up`). Regression: `never_opens_an_exhausted_iterator`, whose iterators panic on an `open` at their end. Until 2026-11-02 this page said the reverse, that LFTJ relies on `open` after `at_end`; only the hand-stepped test `more_complicated` did, by calling `triejoin_open` on an exhausted depth, which no driver does (it is now `triangle_stepped_by_hand`).
- **Constants and repeated variables never reach the join.** `kermit::db::prepare` turns each body atom into an access ([`extract_accesses`](../../kermit-algos/src/access.rs)): a constant `c<id>` becomes a constant filter on its column, and a variable's second and later occurrences in one atom become equalities with its first. The build applies that filter while it copies the rows ([`TupleView`](../../kermit-iters/src/tuple_view.rs)), so LFTJ joins plain indexes, one per distinct (relation, filter, column order), whose columns are all distinct variables: the literature's one-variable-per-level assumption holds, and no iterator wrapper stands between LFTJ and a trie. An atom of constants only builds no index: it is a check whose row count, clamped to 1 on structures that keep no duplicates (both sorted tries), multiplies the results (`Indexes::multiplicity`), so a zero count ends the query. Regression: the `diagonal` and `repeated_nonadjacent` patterns of the standard suite, and `kermit/tests/all_orders.rs`. (Before per-query index builds, 2026-10-22, a one-value `Const_c<id>` relation and a `Select_<n>_<base>` equality view wrapped the iterators instead.)
- **Placeholders never reach the join.** The entry point runs `rewrite_placeholders` ([placeholder_rewrite.rs](../../kermit-algos/src/placeholder_rewrite.rs)) first, so every `_` arrives as its own fresh variable and each atom carries one variable per column — the iterator opens one physical level per variable, so a skipped `_` used to shift the later columns onto the wrong level (`r(X, _, Y)` bound `Y` to the second column; issue #73). Regression: the `trailing_placeholder` and `middle_placeholder` patterns of the standard suite.
- **Valid global attribute order (GAO).** Each iterator descends one stored column per `open()`, so the plan must bind every index's variables in its column order. `join_for_each` checks the plan with [`QueryPlan::validate`](../../kermit-algos/src/optimiser/plan.rs) and panics on an invalid one. Every plan an optimiser makes is valid, because the indexes follow the plan: the planner is free, and [`plan_indexes`](../../kermit-algos/src/index_plan.rs) sorts each access's kept columns by the plan positions of their variables. So a cyclic pair such as `r(X, Y), s(Y, X)` plans like any other query, reading one of the two through an index with its columns swapped, and a constant (`p(c, X)`) is a build-time filter rather than a variable with a fixed place in the order (`kermit/tests/subject_position_constant.rs` and the LUBM cardinality test guard it end to end). The triejoin yields each tuple in descent order; `join_for_each` scatters it into one scratch row in head-first order (lent by `TrieIteratorWrapper::advance`, so no tuple is allocated), so output columns do not depend on the plan. The hash-trie join shares the same `QueryPlan` contract. (Before per-query index builds, 2026-10-22, stored column orders constrained the plan, a cyclic constraint set was rejected with `JoinError::CyclicAttributeOrder`, and a naive first-appearance order silently returned nothing for subject-position constants.)

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

Result tuple: `(7, 4, 5)`. This is the test [`triangle_join_collect`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L509); [`triangle_stepped_by_hand`](../../kermit-algos/src/sorted/leapfrog_triejoin.rs#L371) steps the same join one depth at a time.

## See also

- [`LeapfrogJoin`](./leapfrog-join.md) — the inner k-way intersection used at every depth.
- [`TreeTrie`](../data-structures/tree-trie.md), [`ColumnTrie`](../data-structures/column-trie.md) — index structures that satisfy the `TrieIterable` contract.
- [`docs/optimisers/`](../optimisers/) — the `QueryOptimiser` implementations that plan the `QueryPlan` this algorithm executes.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — the 16 standard patterns LFTJ is tested against under every index structure (Priorities item 1).
