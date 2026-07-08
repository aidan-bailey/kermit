# Cardinality Optimiser

Smallest-relation-first: among the variables whose column-order
constraints are satisfied, bind next the one appearing in the smallest
relation. A variable occurring in a small relation can take only few
values, so binding it early shrinks the search space at every deeper
level. This is the classic size-driven heuristic for worst-case-optimal
joins; prefer it when relation sizes are skewed.

## Ranking function

`rank(v) = min_size(v)` where `min_size(v)` is the minimum tuple count
over the body predicates that mention `v`. The canonical index acts as
the final tie-break (supplied contractually by
`kermit_algos::topological_order`'s heap key, not by the policy), keeping
plans deterministic for reproducible benchmarks.

## Statistics consumed

`CatalogStats::tuples(name)` per body predicate. Relations without an
entry rank as `usize::MAX` ("assume large"). Synthetic `Const_*`
predicates (const-view rewrite) are recorded with one tuple, so constants
bind as early as constraints allow — no special-casing needed.

## Complexity

O(P · a) to compute per-variable minimum sizes, plus O(E + V log V) for
Kahn's sort (a = max predicate arity).

## Worked micro-example

Query: `Q(A, B, C) :- R(A, B), S(A, C).` with |R| = 1000, |S| = 3.
Canonical indices: `A = 0`, `B = 1`, `C = 2`. Constraint edges: `0 -> 1`
(from R) and `0 -> 2` (from S). Per-variable minimum sizes:
`A -> min(1000, 3) = 3`, `B -> 1000`, `C -> 3`.

1. Ready set: `{A}`. Emit `0`.
2. Both edges release `B` and `C`. Ready ranks: `B = 1000`, `C = 3`.
   `C` wins. Emit `2`.
3. Emit `1`.

`variable_ordering = [0, 2, 1]` — the small relation's variable is bound
before the large one's, unlike the lexicographic order `[0, 1, 2]`.

## CLI

    kermit bench run triangle -i tree-trie -a leapfrog-triejoin --optimiser cardinality

Bench-report axis: `optimiser: "cardinality"`.
