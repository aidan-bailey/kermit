# Cost-Based Optimiser

> **Status:** experimental · **CLI:** `--optimiser cost-based` · **Implementation:** [`kermit_algos::optimiser::cost_based`](../../kermit-algos/src/optimiser/cost_based.rs)

Chooses the variable order with the lowest *estimated* cost, by dynamic
programming over the sets of bound variables, in the style of System R
(Selinger et al., 1979). Leapfrog Triejoin's work at each depth depends
only on which variables are bound, not on their order, so the cheapest
plan is a shortest path over those sets; the column-order constraints keep
the space small enough to search exactly. It reads per-column distinct
counts as well as tuple counts. Prefer it when a query mixes a selective
constant with small relations unrelated to it: there `cardinality` binds
the small relations first and enumerates their cross product (WatDiv q0020
and q0034, issue #68).

## Cost function

At depth k, LFTJ binds one key per tuple of the join of every atom
projected onto the plan's first k variables, `S_k`. A plan costs the sum
over depths:

    cost(v₁ … vₙ) = Σₖ est(Sₖ)

`est(S)` is the System R estimate of that join's size:

- **Numerator:** over the atoms with a bound column, the size of their
  projection onto the bound columns. A valid plan binds an atom's columns
  left to right, so they are a prefix of length p, and the size is:
  - the relation's tuple count when p is its arity;
  - its first column's distinct count when p = 1;
  - otherwise, the product of the prefix's distinct counts, capped at the
    tuple count (independence).
- **Denominator:** for each bound variable, the distinct counts of every
  column it occupies, except the smallest. This is containment of value
  sets: the variable's values are assumed to lie within the column with
  the fewest.

An empty relation estimates 0. Distinct counts are clamped to at least 1 in
the denominator.

## Search

A forward dynamic programme.
- **Layers.** Layer k holds every k-variable set a valid plan can bind,
  that is, every set closed under the column-order constraints, each with
  its cheapest order.
- **Validity.** A set grows only by a variable whose predecessors are all
  in it, so every plan found is a topological order of the constraint
  DAG. The plan is passed through `kermit_algos::topological_order`,
  ranked by its own positions, and comes back unchanged; a debug assertion
  checks that nothing was repaired.
- **Ties** break towards the lexicographically smaller order of canonical
  indices, so plans are deterministic.

**Budget.** The sets searched are the downward-closed sets of the
constraint DAG, at most 2ⁿ. The largest WatDiv stress template (q0034, 13
variables) has 820 under `--column-orders stored`, and 2¹³ − 1 = 8,191
under `any`, where the DAG has no edges. Past `state_budget` sets (default
2¹⁴ = 16,384), or past 64 variables (sets are `u64` bitmasks), the
optimiser returns `CardinalityOptimiser`'s plan.

## Statistics consumed

`CatalogStats::tuples(name)` and `CatalogStats::distinct(name, column)`,
for every body predicate. So it declares `required_statistics() ==
StatisticsLevel::ColumnDistinct`:
- the engine's `Database` counts each relation's distinct values per
  column once, when it is built (one walk per relation);
- the join entry points refuse to plan from less
  (`JoinError::MissingStatistics`);
- the other optimisers read only tuple counts, so their runs never pay for
  the walk.

Special cases:
- `Const_*` singletons have one tuple and one distinct value.
- `Select_*` views (repeated variables) report their base relation's
  statistics, as upper bounds.
- Missing statistics only arise when the optimiser is called directly with
  a hand-built `CatalogStats`. The tuple count is then "assume large"
  (`usize::MAX`), and each distinct count equals the tuple count.

## Complexity

O(states × (V + P·a)) for the estimates plus O(transitions) comparisons,
with states ≤ `state_budget`, V variables, and P body predicates of arity
at most a. It runs on every join, inside the `iteration` metric. On the
124 `watdiv-stress-100-test-1` templates (release build), planning takes
4.6 µs at the median and 0.59 ms for q0034, the largest search (820
sets). That is under 0.2 % of q0034's 0.36 s `kermit join` on
`column-trie`, and small queries plan in microseconds.

## Worked micro-example

Query `Q(A, B, Y) :- small(A, Y), link(c7, B), owns(B, Y).`, rewritten to
`small(A, Y), link(K0, B), owns(B, Y), Const_c7(K0)`.

**Variables.** Canonical indices: A = 0, B = 1, Y = 2, K0 = 3.

**Constraints.** A → Y, K0 → B, B → Y.

| Relation | Tuples | Distinct col 0 | Distinct col 1 |
|---|---:|---:|---:|
| `small` | 100 | 100 | 10 |
| `link` | 50,000 | 5,000 | 20,000 |
| `owns` | 40,000 | 20,000 | 10 |
| `Const_c7` | 1 | 1 | — |

**Estimates.** For each set a valid plan can bind:

| S | est(S) |
|---|---:|
| {K0} | 5,000 / 5,000 = 1 |
| {A} | 100 |
| {A, K0} | 100 · 5,000 / 5,000 = 100 |
| {K0, B} | 50,000 · 20,000 / (5,000 · 20,000) = 10 |
| {A, K0, B} | 100 · 50,000 · 20,000 / (5,000 · 20,000) = 1,000 |
| {A, K0, B, Y} | 100 · 50,000 · 40,000 / (5,000 · 20,000 · 10) = 200 |

**Plans** reaching the full set:

| Plan | Cost | |
|---|---:|---|
| K0, B, A, Y | 1 + 10 + 1,000 + 200 = **1,211** | chosen |
| K0, A, B, Y | 1 + 100 + 1,000 + 200 = 1,301 | `cardinality`'s: `small` has only 100 tuples |
| A, K0, B, Y | 100 + 100 + 1,000 + 200 = 1,400 | `lexicographic`'s |

`variable_ordering = [3, 1, 0, 2]`. The unit tests
`worked_example_estimates` and `worked_example_binds_the_selective_side_first`
pin these numbers.

## Regressions and limitations

#68 scored a Python prototype of this optimiser (`plan_est_dp`) on all 124
`watdiv-stress-100-test-1` templates with exact binding counts. Against
`cardinality`, it was better on 32, equal on 89 and worse on 3:

| Query | `cardinality` | `cost-based` | |
|---|---:|---:|---|
| q0020 | 9.68e10 | 1.81e4 | fixed |
| q0034 | 2.27e9 | 9.67e5 | fixed |
| q0008 | 3.81e7 | 8.85e7 | 2.32× worse |
| q0360 | 1.34e4 | 1.67e4 | 1.25× worse |
| q0070 | 3.17e3 | 3.21e3 | 1.01× worse |

The workload total falls from 3.85e11 to 2.64e11 bindings. The search is
exact for its cost model, so each regression is an estimate that ranks a
worse plan as cheaper.

**Parity with the prototype.** Given the same statistics (#68's
`relstats.txt`), kermit's plans equal the prototype's on all 124
templates. Against #68's own run, 6 templates differ: those with a repeated
variable, where #68 measured the filtered selection view and kermit reads
its base relation's statistics. Their exact costs, in bindings:

| Query | kermit | #68 prototype | `cardinality` |
|---|---:|---:|---:|
| q0006 | 141 | 141 | 282 |
| q0031 | 0 | 0 | 0 |
| q0052 | 3 | 3 | 3 |
| q0071 | 57,750 | 57,750 | 77,599 |
| q0091 | 2,378 | 2,266 | 5,694 |
| q0259 | 16 | 15 | 16 |

**On real data.** `kermit join` on q0020 with `--optimiser cost-based`
finishes in 0.09–0.33 s per cell, loading included, where `cardinality`
still runs past 60 s; on q0034 it returns the same 243,150 rows as
`lexicographic` in 0.36 s against 1.76 s (`column-trie`, release build).

`kermit-algos/tests/cost_based_watdiv_plans.rs` pins q0020, q0034 and
q0008 against the committed statistics
(`kermit-algos/tests/fixtures/watdiv-stress-100-stats.tsv`).

**Limitations:**
- **Subject-first tries.** Every relation is stored subject-first, so
  under `--column-orders stored` 10 of #68's 15 slow templates cost 3.7e7
  to 1.7e11 bindings under *every* valid plan. `--column-orders any` lifts
  that ceiling per query by reading a disagreeing atom through a reordered
  copy (see "Column orders"); #82 is to keep selected copies across
  queries, so the copies stop being a per-query cost.
- **Selection views** use their base relation's statistics (cost: the
  table above).
- **Multi-column prefixes** assume independent columns. No WatDiv or LUBM
  relation has more than two columns.
- **Past the budget**, the plan is `cardinality`'s, not an approximation of
  this cost model.

## Column orders

`--column-orders stored` (the default) pins every atom to its stored
column order: the plan binds each atom's columns left to right, as every
plan did before issue #93. `--column-orders any` pins nothing
(`CatalogStats::is_pinned`, read through `Precedence::for_query`), so the
estimate alone decides, and an atom whose plan disagrees with its stored
order is read through a reordered copy
(`docs/specs/2026-10-05-column-orders-design.md`).

Under `any` every subset of the variables is a DP state (2ⁿ − 1), so the
default budget covers up to 14 variables and the search falls back to
`cardinality` above. None of the 124 `watdiv-stress-100-test-1` templates
exceeds it: counting each constant as the variable the rewrite gives it,
the largest has 13 variables (8,191 states). The estimate reads the
distinct counts of an atom's bound columns as a set, which under `stored`
is always a prefix, so plans under `stored` are unchanged
(`cost_based_watdiv_plans.rs` still pins them).

**On real data.** The 10 templates `stored` leaves above 3.7e7 bindings
(q0264, q0079, q0017, q0030, q0306, q0409, q0035, q0010, q0008, q0085)
under `--optimiser cost-based --column-orders any`, through `kermit join`
on the release build: all 30 template × cell runs (tree-trie, column-trie,
hash-trie) return DuckDB's counts, each in under 1 s of wall time, loading
included. Under `stored`, with the same optimiser on tree-trie, 8 of the
10 run past 60 s; q0008 takes 10.9 s and q0085 7.9 s.

## CLI

    kermit bench run watdiv-stress-100-test-1 -i column-trie -a leapfrog-triejoin --optimiser cost-based

Bench-report axis: `optimiser: "cost-based"`. Its `end_to_end` includes
the statistics walk. Its `iteration` does not, because the engine is built
before timing starts.

## See also

- [`LexicographicOptimiser`](./lexicographic.md) and [`CardinalityOptimiser`](./cardinality.md): the tuple-count-only siblings, and the control arms when ablating this one.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md), [`HashTriejoin`](../algorithms/hash-triejoin.md): the algorithms that execute the produced `QueryPlan`.
- [`docs/specs/2026-10-05-cost-based-optimiser-design.md`](../specs/2026-10-05-cost-based-optimiser-design.md): the design, including the catalog (`Database`) it reads from.
- [`docs/specs/optimization-standard.md`](../specs/optimization-standard.md): the optimiser is a first-class benchmark dimension with its own `optimiser` report axis.
