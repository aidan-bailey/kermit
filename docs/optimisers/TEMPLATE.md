# <Optimiser Name>

> **Status:** [stable | experimental] · **CLI:** `--optimiser <name>` · **Implementation:** [`kermit_algos::optimiser::<module>`](../../kermit-algos/src/optimiser/<file>.rs)

One-paragraph summary: what ordering policy this optimiser implements and
when to prefer it.

## Ranking function

The key the policy assigns each candidate variable, and how ties break.
Every optimiser orders variables within Kahn's topological sort
(`kermit_algos::topological_order`) over `Precedence::free`, an edge-free
precedence: no column order constrains the plan, because each index is
built in plan order. So every produced plan is valid by construction —
the policy only chooses among all orders.

## Statistics consumed

Which `CatalogStats` fields the policy reads (or "none"), keyed by access id (`@0`, `@1`, …). A filtered access's counts are
exact, from a scan that `prepare` makes.

## Complexity

Cost of `plan()` in terms of variables V and body atoms P (there are no
constraint edges).

## Worked micro-example

A small query (plus stats if consumed) and the resulting
`variable_ordering`, step by step.

## Index orders

How the policy behaves when the plan is free: no atom is pinned to its
stored column order (`Precedence::free`), so the ranking alone decides,
and `plan_indexes` builds each atom's index with its columns in plan
order. What the plan becomes for a variable bound by an atom's later
column, and any limit the larger search space hits.

## CLI

    kermit bench run <bench> -i <ds> -a <algo> --optimiser <name>

Bench-report axis: `optimiser: "<name>"`.

## See also

- Cross-references to the sibling optimiser docs in `docs/optimisers/`.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md), [`HashTriejoin`](../algorithms/hash-triejoin.md) — the algorithms that execute the produced `QueryPlan`.
- [`docs/specs/optimization-standard.md`](../specs/optimization-standard.md) — the optimiser is a first-class benchmark dimension with its own `optimiser` report axis, distinct from the `ds_layout_*`/`algo_*` optimization axes.
