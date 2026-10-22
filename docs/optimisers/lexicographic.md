# Lexicographic Optimiser

> **Status:** stable · **CLI:** `--optimiser lexicographic` (default) · **Implementation:** [`kermit_algos::optimiser::lexicographic`](../../kermit-algos/src/optimiser/lexicographic.rs)

The default ordering policy: always bind the variable with the smallest
canonical index next. It is the stats-free control: the plan depends on
the query text alone. Before per-query index builds (2026-10-22) it
reproduced, bit for bit, the ordering kermit hardcoded before query
optimisers existed (`global_attribute_order`), Kahn's sort constrained by
each atom's stored column order, so benchmarks planned with it were
directly comparable with pre-optimiser measurements. Nothing is
constrained now, so the plan is the canonical order itself, and it differs
from that earlier order wherever a stored column order forced another (a
subject-position constant, a cyclic pair of atoms). Prefer it as the
control arm of optimiser ablations (e.g. against the
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

O(V log V) for Kahn's sort with a binary-heap ready set over an edge-free
precedence; there are no constraint edges to build.

## Worked micro-example

Query: `Q(Y, X) :- r(X, Y).` Canonical indices: `Y = 0`, `X = 1` (head
variables first, in head order). There are no constraint edges, so the
ready set is `{0, 1}`.

1. Emit `0` (`Y`), the smaller index.
2. Emit `1` (`X`).

`variable_ordering = [0, 1]` — `Y` binds first although it is `r`'s second
column. `r` is the access `@0` (no filter, both columns kept, variables
`[X, Y]`), and its index is built with its columns swapped,
`columns = [1, 0]`, so the join reads `r` in the order it binds it.

## Index orders

The plan is the canonical order itself: head variables in head order, then
body-only variables by first appearance. No atom is pinned to its stored
column order (`Precedence::free`), and `plan_indexes` builds each access's
index with its columns in plan order
(`docs/specs/2026-10-22-per-query-index-build-design.md`). Before that
design, a flag pinned every atom to its stored order by default and lifted
the pin through reordered copies
(`docs/specs/2026-10-05-column-orders-design.md`); the flag is gone.

## CLI

    kermit bench run triangle -i tree-trie -a leapfrog-triejoin --optimiser lexicographic

Bench-report axis: `optimiser: "lexicographic"` (also the default when the
flag is omitted).

## See also

- [`CardinalityOptimiser`](./cardinality.md) — the statistics-driven sibling policy; this optimiser is its control arm in ablations.
- [`CostBasedOptimiser`](./cost-based.md) — plans from per-column distinct counts by dynamic programming over bound-variable sets.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md), [`HashTriejoin`](../algorithms/hash-triejoin.md) — the algorithms that execute the produced `QueryPlan`.
- [`docs/specs/optimization-standard.md`](../specs/optimization-standard.md) — the optimiser is a first-class benchmark dimension with its own `optimiser` report axis, distinct from the `ds_layout_*`/`algo_*` optimization axes.
