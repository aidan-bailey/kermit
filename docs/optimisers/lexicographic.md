# Lexicographic Optimiser

> **Status:** stable · **CLI:** `--optimiser lexicographic` (default) · **Implementation:** [`kermit_algos::optimiser::lexicographic`](../../kermit-algos/src/optimiser/lexicographic.rs)

The default ordering policy: bind the join keys first, then the payload
(#116), and within each group always the variable with the smallest
canonical index next. It is the stats-free control: the plan depends on
the query text alone. Before per-query index builds (2026-10-22) it
reproduced, bit for bit, the ordering kermit hardcoded before query
optimisers existed (`global_attribute_order`), Kahn's sort constrained by
each atom's stored column order, so benchmarks planned with it were
directly comparable with pre-optimiser measurements. Nothing is
constrained now, so the plan is the canonical order of the join keys, then
the canonical order of the payload. It differs from that earlier order
wherever a stored column order forced another (a subject-position
constant, a cyclic pair of atoms), and, since #116, wherever a payload
variable comes canonically before a join key. Prefer it as the
control arm of optimiser ablations (e.g. against the
[cardinality optimiser](./cardinality.md)) and whenever no statistics are
available.

## Ranking function

`rank(v) = v` — the canonical variable index (head variables first, then
body-only variables in first-appearance order, per
`kermit_algos::analyse`), applied by `order_by_rank` within each group:
the join keys, then the payload (see [Payload last](#payload-last-116)).
No tie-break is needed: the rank is already unique per variable. Keeps
head variables early within their group.

## Payload last (#116)

A *join key* is a variable that two or more atoms of the access query
carry (`QueryAnalysis::join_keys`; an atom that carries a variable twice
counts once). Every other variable is *payload*: one atom holds it, so no
intersection constrains it. Every optimiser orders the join keys first
and the payload after them: `order_by_rank` sorts by `(payload?, rank,
index)`, so Lexicographic, [Cardinality](./cardinality.md) and any
optimiser built on it plan payload last, and
[cost-based](./cost-based.md) searches the join keys only. This is step 1
of #116 (`docs/specs/2026-11-02-join-key-levels-design.md`): a HashTrie
under `--ds-layout-levels keys` has no level for a payload column, which a
trie can drop only after its last join key.

What it changes, for a fixed order of the join keys:

- **It never visits more key bindings.** A payload-last search reaches
  each binding of a key prefix once; an interleaved one may reach it once
  per binding of the payload before it.
- **It is never worse under `keys`:** the payload is no level, and the
  leaf product enumerates only the result rows.
- **Under `all` and on the sorted tries it adds a payload walk,**
  proportional to the output, that a payload-first plan can partly avoid.
  In `Q(P, K) :- R(P, K), S(K).`, with R holding 1,000 rows under one P
  value and S the same 1,000 K values, the plan P, K takes 1 scan step at
  P and 1,000 scans plus 1,000 lookups at K (2,001 steps); the payload-last
  K, P takes the same 2,000 at K and 1,000 scans of one-bucket P tables
  (3,000 steps), about 1.5× more under `all`, and 2,000 under `keys`.
- **It can split an index a self-join shared.** SNAP's `path3`,
  `P(A, B, C, D) :- edge(A, B), edge(B, C), edge(C, D).`, has join keys B
  and C and payload A and D. Under the order A, B, C, D all three atoms
  read `edge` as `[0, 1]`, one index; under the payload-last B, C, A, D,
  `edge(A, B)` is read destination first, `[1, 0]`: two `edge` indexes, on
  every structure. SNAP's triangle, cycle4 and clique4 have no payload.

Every benchmark query with a payload variable may change plan. Report v6's
`plan` axis records each row's plan, so those rows are told apart without
a schema bump.

## Statistics consumed

None. `plan()` ignores `CatalogStats` entirely.

## Complexity

O(V log V) to sort the variables by rank (`kermit_algos::order_by_rank`).

## Worked micro-example

Query: `Q(Y, X) :- r(X, Y).` Canonical indices: `Y = 0`, `X = 1` (head
variables first, in head order), and each variable ranks by its index:

1. `0` (`Y`), the smaller index, binds first.
2. `1` (`X`).

`variable_ordering = [0, 1]` — `Y` binds first although it is `r`'s second
column. `r` is the access `@0` (no filter, both columns kept, variables
`[X, Y]`), and its index is built with its columns swapped,
`columns = [1, 0]`, so the join reads `r` in the order it binds it. (One
atom carries both variables, so both are payload, and the payload keeps
canonical order.)

With a payload variable among join keys: `Q(X, P, Y) :- R(X, P), S(X, Y),
T(Y).` numbers `X = 0`, `P = 1`, `Y = 2`. X and Y are join keys and P is
payload, so `variable_ordering = [0, 2, 1]`: X, Y, then P.

## Index orders

The plan is the canonical order of the join keys, then of the payload:
within each, head variables in head order, then body-only variables by
first appearance. No atom is pinned to its stored
column order, and `plan_indexes` builds each access's
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
