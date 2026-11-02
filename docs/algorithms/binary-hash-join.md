# `BinaryHashJoin`

> **Status:** experimental · **CLI:** `-a binary-hash-join` (pairs only with `-i hash-index`) · **Implementation:** [`kermit_algos::BinaryHashJoin`](../../kermit-algos/src/keyed/binary_hash_join.rs)

## What it is

A left-deep, pipelined binary hash join: the conventional join plan, and the baseline for what worst-case optimal joins buy. It is the plan of Wang, Willsey and Suciu, "Free Join: Unifying Worst-Case Optimal and Traditional Joins" (SIGMOD 2023), §2.2: the engine iterates over each tuple of the first atom, probes the hash table of the second on the variables the first binds, and for each match probes the third, and so on. Nothing intermediate is stored, so it fits `join_for_each`'s streaming contract unchanged.

It is not worst-case optimal. Its work is proportional to the sum of the sizes of its intermediate results, and on a cyclic query an intermediate result can exceed the AGM bound that [`HashTriejoin`](hash-triejoin.md) and [`LeapfrogTriejoin`](leapfrog-triejoin.md) meet. That gap is what this cell measures.

It runs over the third iterator family, `KeyedIterable` ([`HashIndex`](../data-structures/hash-index.md)): an index keyed on its leading columns, probed by exact match. Its join order comes from the optimiser's variable order, not from a binary-join optimiser (see "Where the join order comes from").

## Pseudocode

Free Join §2.2, Figure 2a, for the triangle `Q(x, y, z) :- R(x, y), S(y, z), T(x, z)` with the plan `[R, S, T]`:

```
for (x, y) in R:
    for (y, z) in S[y]:          # a failed lookup continues the loop above
        for (x, z) in T[x, z]:
            output(x, y, z)
```

The general form this crate runs. Step `j` is the `j`-th body atom of the executed query, with its index `I_j`, its variables `x_j` in column order, and `k_j = key_len(I_j)`:

```
join_for_each(plan, query, indexes, emit):
    validate plan against analyse(query)
    steps ← [(I_j, x_j, k_j) for each body atom j, in body order]
    assert for each step j:  x_j[..k_j] are bound by steps before j
                             x_j[k_j..] are not
    row ← scratch[num_vars];  keys ← scratch[Σ k_j]      # allocated once per join
    descend(0)

descend(j):
    if j = |steps|: emit(row); return
    key ← row[x_j[..k_j]]
    for t in I_j.probe(key):             # k_j = 0: every row, in source order
        row[x_j[i]] ← t[i]   for i in k_j..arity_j
        descend(j + 1)
```

Rust mapping, in [`kermit-algos/src/keyed/binary_hash_join.rs`](../../kermit-algos/src/keyed/binary_hash_join.rs):

- `join_for_each` → `<BinaryHashJoin as JoinAlgo<DS>>::join_for_each`, which validates the plan, builds the steps and calls `descend`.
- One loop level → `descend(steps, row, keys, emit)`. It splits `keys` into this level's key and the deeper levels' slots, so a level's key stays intact while the levels below it run.
- A step and the invariant check → `Step::new`.

**Where the join order comes from.** `join_for_each` reads the executed body left to right and derives nothing. [`plan_indexes`](../../kermit-algos/src/index_plan.rs) under `TrieLevels::ProbeKeys` does the deriving, by the rule of [`join_order.rs`](../../kermit-algos/src/join_order.rs), which reads a variable order as a sequence of binding events: each variable of the order that is not yet bound joins the atom that binds it (the unjoined atom containing it with the most variables already bound, ties to the earliest in the body), and an atom whose variables are then all bound is probed at once. The order `V′` it returns lists each joined atom's new variables after its key variables, so an index whose columns follow `V′` has its key first, and `IndexSpec::levels` is the key length: `0` for the driver (the first atom) and for an atom joined across a cross product.

## Invariants

- **Probe keys lead the columns.** For every step `j`, the first `key_len` variables of its atom are bound by the steps before `j`, and none of the others is bound yet. `Step::new` asserts it (the panic message starts with `probe keys lead the columns`), in the role LFTJ's ascent assertion plays. If it broke, a key column no earlier atom binds would probe with a stale value from the scratch row, and a column past the key that was already bound would be overwritten instead of joined on, so the join would return wrong rows silently. It catches a query not planned under `ProbeKeys` (for example, indexes keyed on every column, as `bench ds` builds them); `kermit::db::build` also refuses an index set planned under another trie-level policy, one layer up. Pinned by `an_index_keyed_on_every_column_is_refused` and `a_key_no_earlier_atom_binds_is_refused`.
- **The body order is the join order.** Step `j` is body atom `j`. `plan_indexes` lists the executed body in join order, with `#0` the driver. Reordering the body without re-planning the keys breaks the invariant above.
- **Constants and repeated variables never reach the join.** `kermit::db::prepare` turns each body atom into an access, and the build applies its filter while it copies the rows into the index, so every atom here has distinct variables, one per column. An atom of constants only builds no index: its row count multiplies the results on a structure that keeps duplicates (`Relation::KEEPS_DUPLICATES`, true for `HashIndex`), and a zero count ends the query. A placeholder arrives as its own fresh variable (`rewrite_placeholders`).
- **Every variable of the query is emitted, in canonical order.** `row[v]` is canonical variable `v` (head variables first). `kermit::db::execute` lends the head prefix `&row[..head_len]`, so body-only variables never reach the caller. A variable no atom carries has no value, so `join_for_each` returns before descending (validation rules it out for every query from `kermit::db`).
- **The result is streamed and the join allocates once.** `emit(row)` lends the one scratch row. The row, the key scratch and the steps are allocated once per join, and `probe` allocates nothing, so emitting a tuple, or descending, allocates nothing (`kermit/tests/result_allocation.rs`: result size, permuted index, descent count and collected cells).
- **Bag semantics.** `HashIndex` lends each duplicate row separately, so a duplicated row on either side of a join multiplies its matches, as on `HashTrie`. The cell's counts equal `HashTriejoin`'s on every query.
- **The plan is validated.** `join_for_each` calls `QueryPlan::validate` and panics on an invalid plan (a permutation of the query's variables whose order respects every atom's column order). Every plan `plan_indexes` returns under `ProbeKeys` validates, since each atom lists its variables in `V′` order.

## Complexity

Let `a` be the number of body atoms, `v` the number of variables, and `P_j` the number of partial results that reach step `j` (`P_0 = 1`, and `P_a` is the output size before head projection).

| Operation | Time | Notes |
|---|---|---|
| `join_for_each` setup | O(a + v) | One `analyse`, the steps, the row and the key scratch; none per result row. |
| One probe (`probe`) | O(1) expected, O(rows) worst | Directory load factor ≤ 1 and Fibonacci bucket selection; the key comparison filters other keys' rows from a chain, so a degenerate hash costs chain length, never correctness. |
| `join_for_each`, total | O(Σ_{j≥1} P_{j−1} · probe + Σ_j P_j) | The sum of the intermediate results: each partial result probes the next atom once, and each match is written once. |

There is no worst-case bound in the input size alone. On the triangle with `|R| = |S| = |T| = N` the AGM bound is `N^{3/2}`, which worst-case optimal joins meet, but the first intermediate result `|R ⋈ S|` can reach Θ(N²). The standard instance: `R = S = T = {(0, j), (j, 0) : 1 ≤ j ≤ m}`, so `N = 2m`. Then `R ⋈ S` on the middle variable holds `m² + m` tuples (every `(i, 0)` of `R` with every `(0, k)` of `S`, plus the `m` tuples `(0, j, 0)`) and the output is empty, since `T` has no tuple `(i, k)` with both values non-zero. At `m = 100` that is 10 100 probes into `T` against `N^{3/2} ≈ 2 828`.

## Worked micro-example

The triangle with the variable order `[X, Y, Z]` and

```
R(X, Y) = {(1, 2), (1, 3), (2, 3)}
S(Y, Z) = {(2, 3), (3, 4), (3, 5)}
T(X, Z) = {(1, 3), (1, 4), (2, 5)}
```

`prepare` under `ProbeKeys` applies the rule to `V = [X, Y, Z]`. The first unbound variable is `X`; `R` and `T` both contain it with nothing bound, so the earliest in the body, `R`, joins first and binds `X` and `Y`. No other atom is fully bound, so the next unbound variable is `Z`; `S` and `T` both contain it with one bound variable each, so `S` joins second, keyed on `Y`. `T` is now fully bound, so the filter step joins it third, keyed on `(X, Z)`. The executed query is `Q(X, Y, Z) :- #0(X, Y), #1(Y, Z), #2(X, Z)` with `V′ = [X, Y, Z]`, and the indexes are keyed as `#0`: no key (`levels` 0), `#1`: `Y` (1), `#2`: `(X, Z)` (2).

| Step | Loop | Probe | Matches |
|---|---|---|---|
| `#0` | `(X, Y)` over every row of `R` | key `()` | `(1,2)`, `(1,3)`, `(2,3)` |
| `#1` | for `(1,2)`: `Z` from `S[Y=2]` | key `(2)` | `(2,3)`, so `Z = 3` |
| `#2` | for `(1,2,3)`: `T[X=1, Z=3]` | key `(1,3)` | hit: emit `(1,2,3)` |
| `#1` | for `(1,3)`: `S[Y=3]` | key `(3)` | `(3,4)`, `(3,5)` |
| `#2` | for `(1,3,4)`: `T[1,4]` | key `(1,4)` | hit: emit `(1,3,4)` |
| `#2` | for `(1,3,5)`: `T[1,5]` | key `(1,5)` | miss |
| `#1` | for `(2,3)`: `S[Y=3]` | key `(3)` | `(3,4)`, `(3,5)` |
| `#2` | for `(2,3,4)`: `T[2,4]` | key `(2,4)` | miss |
| `#2` | for `(2,3,5)`: `T[2,5]` | key `(2,5)` | hit: emit `(2,3,5)` |

`|R ⋈ S|` is 5 (the five probes into `T`) and the output is `{(1,2,3), (1,3,4), (2,3,5)}`. This is the unit test `the_triangle_probes_each_atom_on_what_the_earlier_ones_bind` in `binary_hash_join.rs`; the same query runs end to end in `kermit/src/db.rs`'s `the_keyed_family_answers_like_the_hash_family`.

## See also

- [`HashIndex`](../data-structures/hash-index.md): the keyed index this algorithm probes.
- [`HashTriejoin`](hash-triejoin.md) and [`LeapfrogTriejoin`](leapfrog-triejoin.md): the worst-case optimal cells this one is the baseline for.
- `kermit_algos::plan_indexes` ([`index_plan.rs`](../../kermit-algos/src/index_plan.rs)) and `join_order.rs` ([`join_order.rs`](../../kermit-algos/src/join_order.rs)): the `ProbeKeys` planning that produces the executed query.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; this algorithm passes all 16 patterns over `HashIndex` under every optimiser (`kermit/tests/join_tests.rs`), and `kermit/tests/all_orders.rs` runs every shape under every variable order.
