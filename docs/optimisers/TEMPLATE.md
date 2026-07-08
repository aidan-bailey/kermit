# <Optimiser Name>

One-paragraph summary: what ordering policy this optimiser implements and
when to prefer it.

## Ranking function

The key the policy assigns each candidate variable, and how ties break.
Every optimiser orders variables within Kahn's topological sort over the
column-order constraint DAG (`kermit_algos::topological_order`), so every
produced plan is valid by construction — the policy only chooses among
valid orders.

## Statistics consumed

Which `CatalogStats` fields the policy reads (or "none").

## Complexity

Cost of `plan()` in terms of variables V, body predicates P, and
constraint edges E.

## Worked micro-example

A small query (plus stats if consumed), the constraint DAG, and the
resulting `variable_ordering`, step by step.

## CLI

    kermit bench run <bench> -i <ds> -a <algo> --optimiser <name>

Bench-report axis: `optimiser: "<name>"`.
