# Lexicographic Optimiser

> **Status:** stable · **CLI:** `--optimiser lexicographic` (default) · **Implementation:** [`kermit_algos::optimiser::lexicographic`](../../kermit-algos/src/optimiser/lexicographic.rs)

The default ordering policy: among the variables whose column-order
constraints are satisfied, always bind the one with the smallest canonical
index next. Reproduces, bit for bit, the ordering kermit hardcoded before
query optimisers existed (`global_attribute_order`), so benchmarks planned
with it are directly comparable with pre-optimiser measurements. Prefer it
as the control arm of optimiser ablations (e.g. against the
[cardinality optimiser](./cardinality.md)) and whenever no statistics are
available.

## Ranking function

`rank(v) = v` — the canonical variable index (head variables first, then
body-only variables in first-appearance order, per
`kermit_algos::analyse`). No tie-break is needed: the rank is already
unique per variable. Keeps head variables early when unconstrained.

## Statistics consumed

None. `plan()` ignores `CatalogStats` entirely.

## Complexity

O(E + V log V) for Kahn's sort with a binary-heap ready set, plus O(P · a)
to build the constraint edges (a = max predicate arity).

## Worked micro-example

Query: `Q(Y) :- r(K, Y), Const_c7(K).` (the subject-position-constant
shape). Canonical indices: `Y = 0` (head), `K = 1`. Constraint DAG:
`r(K, Y)` contributes the edge `1 -> 0`.

1. Ready set: `{1}` (only `K` has in-degree 0). Emit `1`.
2. Edge `1 -> 0` releases `0`. Ready set: `{0}`. Emit `0`.

`variable_ordering = [1, 0]` — the constant's variable binds first even
though the head variable is canonically smaller.

## CLI

    kermit bench run triangle -i tree-trie -a leapfrog-triejoin --optimiser lexicographic

Bench-report axis: `optimiser: "lexicographic"` (also the default when the
flag is omitted).

## See also

- [`CardinalityOptimiser`](./cardinality.md) — the statistics-driven sibling policy; this optimiser is its control arm in ablations.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md), [`HashTriejoin`](../algorithms/hash-triejoin.md) — the algorithms that execute the produced `QueryPlan`.
- [`docs/specs/optimization-standard.md`](../specs/optimization-standard.md) — the optimiser is a first-class benchmark dimension with its own `optimiser` report axis, distinct from the `ds_layout_*`/`algo_*` optimization axes.
