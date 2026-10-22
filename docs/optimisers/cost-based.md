# Cost-Based Optimiser

> **Status:** experimental · **CLI:** `--optimiser cost-based` · **Implementation:** [`kermit_algos::optimiser::cost_based`](../../kermit-algos/src/optimiser/cost_based.rs)

Chooses the variable order with the lowest *estimated* cost, by dynamic
programming over the sets of bound variables, in the style of System R
(Selinger et al., 1979). Leapfrog Triejoin's work at each depth depends
only on which variables are bound, not on their order, so the cheapest
plan is a shortest path over those sets, searched exactly while their
number stays within a budget. It reads per-column distinct
counts as well as tuple counts. Prefer it when a query mixes a selective
constant with small relations unrelated to it: #68 found that `cardinality` bound
the small relations first and enumerated their cross product (WatDiv q0020
and q0034).

## Cost function

At depth k, LFTJ binds one key per tuple of the join of every atom
projected onto the plan's first k variables, `S_k`. A plan costs the sum
over depths:

    cost(v₁ … vₙ) = Σₖ est(Sₖ)

`est(S)` is the System R estimate of that join's size:

- **Numerator:** over the atoms with a bound column, the size of their
  projection onto the bound columns. An access's index is built in plan
  order, so they are a prefix of length p of its index, and the size is:
  - the access's tuple count when every kept column is bound;
  - the distinct count of the bound column when p = 1;
  - otherwise, the product of the bound columns' distinct counts, capped at
    the tuple count (independence).
- **Denominator:** for each bound variable, the distinct counts of every
  column it occupies, except the smallest. This is containment of value
  sets: the variable's values are assumed to lie within the column with
  the fewest.

An empty relation estimates 0. Distinct counts are clamped to at least 1 in
the denominator.

## Search

A forward dynamic programme.
- **Layers.** Layer k holds every k-variable set of the query's variables,
  each with its cheapest order. No order constrains the plan, so every
  subset is a state.
- **Validity.** A set grows by any variable not yet in it, so every plan
  found is a permutation of the variables. The plan is passed through
  `kermit_algos::topological_order` over `Precedence::free`, ranked by its
  own positions, and comes back unchanged; a debug assertion checks that
  nothing was repaired.
- **Ties** break towards the lexicographically smaller order of canonical
  indices, so plans are deterministic.

**Budget.** The sets searched are all subsets of the query's variables,
2ⁿ − 1 of them. A constant is a filter on its access, not a variable, so
only distinct variables count. The largest WatDiv stress template (q0034)
had 13 variables when each constant counted as one, which gave
2¹³ − 1 = 8,191 sets; it has 12 now (4,095 sets), as does the largest of the
12,400 committed queries. Past `state_budget` sets
(default 2¹⁴ = 16,384), or past 64 variables (sets are `u64` bitmasks), the
optimiser returns `CardinalityOptimiser`'s plan.

## Statistics consumed

`CatalogStats::tuples(name)` and `CatalogStats::distinct(name, column)`,
for every access (keyed by access id, `@0`, `@1`, …). So it declares
`required_statistics() == StatisticsLevel::ColumnDistinct`:
- the `Catalog` counts each relation's distinct values per column once,
  when it is built (one walk per relation), outside every timer;
- `prepare` refuses to plan from less (`JoinError::MissingStatistics`);
- an unfiltered access reads its relation's catalog statistics, and a
  filtered access (a constant, or a repeated variable) is scanned through
  its filter in `prepare`, for exact tuple and per-column distinct
  counts, so a constant's selectivity is measured, not estimated;
- the other optimisers read only tuple counts, so their runs never pay for
  the distinct walk.

Special cases:
- A constant is a filter, not a variable: its access has fewer columns, and
  its counts are those of the rows that match.
- A repeated variable is an equality in the filter, so its counts are exact
  too (they were the base relation's, as upper bounds, while selection
  views stood in).
- Missing statistics only arise when the optimiser is called directly with
  a hand-built `CatalogStats`. The tuple count is then "assume large"
  (`usize::MAX`), and each distinct count equals the tuple count.

## Complexity

O(states × (V + P·a)) for the estimates plus O(transitions) comparisons,
with states ≤ `state_budget`, V variables, and P body predicates of arity
at most a. It runs once per query, in `prepare`, outside every timer. On
the 124 `watdiv-stress-100-test-1` templates (release build, measured
before per-query index builds, when the plan was constrained by stored
column order), planning took 4.6 µs at the median and 0.59 ms for q0034,
the largest search (820 sets). A free search is larger, up to 2ⁿ − 1 sets,
and is not re-measured here. The 0.59 ms was under 0.2 % of q0034's 0.36 s
`kermit join` on `column-trie`, and small queries plan in microseconds.

## Worked micro-example

Query `Q(A, B, Y) :- small(A, Y), link(c7, B), owns(B, Y).`

**Accesses.** `prepare` reads one access per atom: `@0` is `small(A, Y)`;
`@1` is `link(c7, B)`, with the constant as the filter `col0=7`, so it
keeps only column 1, bound to B; `@2` is `owns(B, Y)`. Canonical indices:
A = 0, B = 1, Y = 2. No constraint orders the variables.

| Access | Reads | Tuples | Distinct values |
|---|---|---:|---|
| `@0` | `small(A, Y)` | 100 | A: 100, Y: 10 |
| `@1` | `link`, filter `col0=7`, column 1 | 12 | B: 12 |
| `@2` | `owns(B, Y)` | 40,000 | B: 20,000, Y: 10 |

`link` itself holds 50,000 tuples. The filtered scan found that 12 of them
have the constant in column 0, so `@1` has 12 tuples: the statistics are
those of the rows that match, not an estimate from the whole relation.

**Estimates.** For each set a plan can bind:

| S | est(S) |
|---|---:|
| {A} | 100 |
| {B} | 12 · 20,000 / 20,000 = 12 |
| {Y} | 10 · 10 / 10 = 10 |
| {A, B} | 100 · 12 · 20,000 / 20,000 = 1,200 |
| {A, Y} | 100 · 10 / 10 = 100 |
| {B, Y} | 10 · 12 · 40,000 / (20,000 · 10) = 24 |
| {A, B, Y} | 100 · 12 · 40,000 / (20,000 · 10) = 240 |

**Plans** reaching the full set:

| Plan | Cost | |
|---|---:|---|
| Y, B, A | 10 + 24 + 240 = **274** | chosen |
| B, Y, A | 12 + 24 + 240 = 276 | |
| Y, A, B | 10 + 100 + 240 = 350 | |
| B, A, Y | 12 + 1,200 + 240 = 1,452 | `cardinality`'s: the 12-tuple access binds first |
| A, B, Y | 100 + 1,200 + 240 = 1,540 | `lexicographic`'s |

`variable_ordering = [2, 1, 0]`. `plan_indexes` then builds `@0` as `small`
with columns `[1, 0]` (Y before A), `@1` as `link` with the filter
`col0=7` and columns `[1]`, and `@2` as `owns` with columns `[1, 0]` (Y
before B).

The unit tests `worked_example_estimates` (the estimate table) and
`worked_example_plans` (each order's cost, the cost-based plan
`[2, 1, 0]` and `cardinality`'s `[1, 0, 2]`) in `cost_based.rs` pin these
numbers, planning over the accesses as `prepare` does. The other unit tests
there and in `cardinality.rs` state their own constant statistics, and
`cost_based_watdiv_plans.rs` pins the real templates' plans (see "Index
orders").

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
exact for its cost model (`the_search_finds_the_cheapest_order`
checks it against brute force on random queries), so each regression is an
estimate that ranks a worse plan as cheaper.

**Parity with the prototype.** Given the same statistics (#68's
`relstats.txt`), kermit's plans equal the prototype's on all 124
templates. Against #68's own run, 6 templates differ: those with a repeated
variable, where #68 measured the filtered selection view and kermit read
its base relation's statistics before per-query index builds. Their exact
costs, in bindings:

| Query | kermit | #68 prototype | `cardinality` |
|---|---:|---:|---:|
| q0006 | 141 | 141 | 282 |
| q0031 | 0 | 0 | 0 |
| q0052 | 3 | 3 | 3 |
| q0071 | 57,750 | 57,750 | 77,599 |
| q0091 | 2,378 | 2,266 | 5,694 |
| q0259 | 16 | 15 | 16 |

Since per-query index builds (2026-10-22), a repeated variable is an
equality in its access's filter, whose statistics are scanned exactly, so
the gap on those 6 templates should close. The table is the earlier run's, with
the base relation's statistics, and is not re-measured here.

**On real data** (2026-10-05, before per-query index builds; not re-measured). `kermit join` on q0020 with `--optimiser cost-based`
finishes in 0.09–0.33 s per cell, loading included, where `cardinality`
still runs past 60 s; on q0034 it returns the same 243,150 rows as
`lexicographic` in 0.36 s against 1.76 s (`column-trie`, release build).

`kermit-algos/tests/cost_based_watdiv_plans.rs` pins q0020, q0034, q0008, q0264 and
q0085 against the committed statistics
(`kermit-algos/tests/fixtures/watdiv-stress-100-stats.tsv`).

**Limitations:**
- **Subject-first tries.** Every relation is stored subject-first. Before
  per-query index builds (2026-10-22), a flag pinned every atom to its
  stored column order by default, and 10 of #68's 15 slow templates cost
  3.7e7 to 1.7e11 bindings under *every* valid plan. The plan is free now
  and each index is built in plan order, so that ceiling is gone; the
  price is that every query builds its own indexes, inside its `build`
  metric (see "Index orders"). #82 is rescoped to an index cache over this
  design, so the builds stop being a per-query cost.
- **Multi-column prefixes** assume independent columns. No WatDiv or LUBM
  relation has more than two columns.
- **Past the budget**, the plan is `cardinality`'s, not an approximation of
  this cost model.

## Index orders

The plan is free: no atom is pinned to its stored column order, so the
estimate alone decides (`Precedence::free`), and `plan_indexes` builds each
access's index with its columns in plan order
(`docs/specs/2026-10-22-per-query-index-build-design.md`). Before that
design, a flag pinned every atom to its stored order by default and lifted
the pin through reordered copies
(`docs/specs/2026-10-05-column-orders-design.md`); the flag is gone.

Every subset of the variables is a DP state (2ⁿ − 1), so the default
budget covers up to 14 variables and the search falls back to
`cardinality` above. Constants are filters, not variables, so they no
longer count: none of the 124 `watdiv-stress-100-test-1` templates exceeds
the budget, and the largest has 12 variables (4,095 states; it had 13 under
the old const rewrite, which counted its constant as a variable).
`kermit-algos/tests/cost_based_watdiv_plans.rs` pins the plans of the
committed templates it names (q0020, q0034, q0008, q0264 and q0085)
against the committed statistics. It has no data to scan, so it estimates
a filtered access's counts as the rows divided by the filtered column's
distinct count. `every_committed_query_fits_the_budget` checks all 12,400
committed queries, and `q0034_fits_the_budget` that the largest plans as
the unbounded search does. The estimate reads the distinct counts of an
access's bound columns as a set, which is always a prefix of its index.

**On real data** (2026-10-06, measured with the column-orders flag, since
removed). The 10 templates the stored order left above 3.7e7 bindings
(q0264, q0079, q0017, q0030, q0306, q0409, q0035, q0010, q0008, q0085)
under `--optimiser cost-based` with every column order allowed, through
`kermit join` on the release build: all 30 template × cell runs (tree-trie,
column-trie, hash-trie) returned DuckDB's counts, each in under 1 s of wall
time, loading included. Under the stored order, with the same optimiser on
tree-trie, 8 of the 10 ran past 60 s; q0008 took 10.9 s and q0085 7.9 s.

## CLI

    kermit bench run watdiv-stress-100-test-1 -i column-trie -a leapfrog-triejoin --optimiser cost-based

Bench-report axis: `optimiser: "cost-based"`. The distinct-count walk runs
only for this optimiser, when the catalog is built in `Workload::prepare`,
and the filtered accesses' scans run in `prepare`; both are outside `build`,
`iteration` and `end_to_end`.

## See also

- [`LexicographicOptimiser`](./lexicographic.md) and [`CardinalityOptimiser`](./cardinality.md): the tuple-count-only siblings, and the control arms when ablating this one.
- [`LeapfrogTriejoin`](../algorithms/leapfrog-triejoin.md), [`HashTriejoin`](../algorithms/hash-triejoin.md): the algorithms that execute the produced `QueryPlan`.
- [`docs/specs/2026-10-05-cost-based-optimiser-design.md`](../specs/2026-10-05-cost-based-optimiser-design.md): the design, including the catalog it reads from (the engine's per-structure store when it was written; the flat `Catalog` since [per-query index builds](../specs/2026-10-22-per-query-index-build-design.md)).
- [`docs/specs/optimization-standard.md`](../specs/optimization-standard.md): the optimiser is a first-class benchmark dimension with its own `optimiser` report axis.
