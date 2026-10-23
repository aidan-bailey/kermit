# Cardinality Optimiser

> **Status:** stable · **CLI:** `--optimiser cardinality` · **Implementation:** [`kermit_algos::optimiser::cardinality`](../../kermit-algos/src/optimiser/cardinality.rs)

Smallest-relation-first: bind next the variable appearing in the smallest
relation. A variable occurring in a small relation can take only few
values, so binding it early shrinks the search space at every deeper
level. This is the classic size-driven heuristic for worst-case-optimal
joins; prefer it when relation sizes are skewed.

## Ranking function

`rank(v) = min_size(v)` where `min_size(v)` is the minimum tuple count
over the accesses (body atoms) that mention `v`. The canonical index acts as
the final tie-break (supplied contractually by
`kermit_algos::order_by_rank`'s sort key, not by the policy), keeping
plans deterministic for reproducible benchmarks.

## Statistics consumed

`CatalogStats::tuples(name)` per access, keyed by access id (`@0`, `@1`,
…). An unfiltered access reads its relation's catalog count; a filtered
one (a constant or a repeated variable) counts the rows that pass its
filter, in a scan that `prepare` makes. A constant therefore
shrinks its relation's count, and its variables bind early, with no
special-casing. Accesses without an entry rank as `usize::MAX` ("assume
large").

## Complexity

O(P · a) to compute per-variable minimum sizes, plus O(V log V) to sort
the variables by them (a = max access arity).

## Worked micro-example

Query: `Q(A, B, C) :- R(A, B), S(A, C).` with |R| = 1000, |S| = 3, both
unfiltered (accesses `@0` and `@1`). Canonical indices: `A = 0`,
`B = 1`, `C = 2`. No variable is constrained. Per-variable minimum sizes:
`A -> min(1000, 3) = 3`, `B -> 1000`, `C -> 3`.

1. Ready set: `{A, B, C}` with ranks 3, 1000, 3. `A` and `C` tie, and the
   canonical index breaks it. Emit `0`.
2. Ready ranks: `B = 1000`, `C = 3`. `C` wins. Emit `2`.
3. Emit `1`.

`variable_ordering = [0, 2, 1]` — the small relation's variable is bound
before the large one's, unlike the [lexicographic](./lexicographic.md)
order `[0, 1, 2]`. Written `S(C, A)` instead, the query gets the same plan,
and `S`'s index is built with its columns swapped, `[1, 0]`.

## Index orders

The plan is free: no atom is pinned to its stored column order, so the
ranking alone decides, and `plan_indexes` builds each
access's index with its columns in plan order
(`docs/specs/2026-10-22-per-query-index-build-design.md`). The smallest
relation's variable therefore goes first even when its column is not that
relation's first. Before that design, a flag pinned every atom to its
stored order by default and lifted the pin through reordered copies
(`docs/specs/2026-10-05-column-orders-design.md`); the flag is gone.

## CLI

    kermit bench run triangle -i tree-trie -a leapfrog-triejoin --optimiser cardinality

Bench-report axis: `optimiser: "cardinality"`.

## See also

- [`LexicographicOptimiser`](./lexicographic.md) — the stats-free sibling policy and default; use it as the control arm when ablating this one.
- [`CostBasedOptimiser`](./cost-based.md) — plans from per-column distinct counts by dynamic programming over bound-variable sets.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md), [`HashTriejoin`](../algorithms/hash-triejoin.md) — the algorithms that execute the produced `QueryPlan`.
- [`docs/specs/optimization-standard.md`](../specs/optimization-standard.md) — the optimiser is a first-class benchmark dimension with its own `optimiser` report axis, distinct from the `ds_layout_*`/`algo_*` optimization axes.
