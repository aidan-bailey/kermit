# `HashTriejoin`

> **Status:** experimental · **CLI:** `-a hash-triejoin` · **Implementation:** [`kermit_algos::HashTriejoin`](../../kermit-algos/src/hash/hash_triejoin.rs)

## What it is

A worst-case-optimal multi-way join over hash-trie-structured relations. Implements Algorithm 3 from "Combining Worst-Case Optimal and Traditional Binary Join Processing" (Freitag et al., SIGMOD 2020) — the probe phase of the paper's hash trie join.

The algorithm has the same asymptotic guarantee as Leapfrog Triejoin (output size bounded by the AGM bound) but uses hash-based intersection (`lookup(h)`) instead of comparison-based intersection (least-upper-bound `seek`). This makes it competitive on workloads where the seek dominates LFTJ's cost — typically large fan-outs.

## Pseudocode

```
function enumerate(i):                              # Alg. 3 line 1
    if i ≤ n:                                       # line 2
        I_join  = {I_j ∈ I | v_i ∈ E_j}             # line 3
        I_other = {I_j ∈ I | v_i ∉ E_j}             # line 4
        I_scan  = argmin_{I_j ∈ I_join} size(I_j)   # line 5
        repeat                                       # line 6
            # Probe other tables
            for I_j ∈ I_join \ I_scan:              # line 7
                if not lookup(I_j, hash(I_scan)):   # line 8
                    skip current iteration          # line 9
            for I_j ∈ I_join: down(I_j)             # lines 10-11
            enumerate(i + 1)                        # line 12
            for I_j ∈ I_join: up(I_j)               # lines 13-14
        until not next(I_scan)                      # line 15
    else:                                            # line 16
        for t ∈ ⨯_{I_j ∈ I_join} tuples(I_j):       # line 17
            if join condition holds for t:           # line 18
                produce(t)                           # line 19
```

Rust mapping:

- `enumerate(i)` → `Probe::enumerate(&mut self, i)` in [`kermit-algos/src/hash/hash_triejoin.rs`](../../kermit-algos/src/hash/hash_triejoin.rs). `Probe` holds what the recursion reads, fixed per join: the iterators (`I`), each atom's variables (`E_j`), `variable_to_iter_map` (`I_join` at each position), the leaf scratch and the sink, so `enumerate` takes the paper's one argument.
- Lines 16–19 → `Probe::emit_leaf`, an odometer of one `LeafCursor` per relation, advanced by `fn advance_cursor(iters, cursor)`.
- Line 18's verification → `fn verify_and_construct(candidate, predicate_variables, row, bound)`, which writes into a per-join scratch row (`LeafScratch`).

**Participation by level** (#116). Line 3's `I_join` is the atoms whose trie has a *level* for `v_i`: `build_variable_to_iter_map` keeps an atom for `v_i` only when `v_i` is among its first `levels()` columns (`HashTrieIterable::levels`, read once per join). Under the default `--ds-layout-levels all` every column is a level, so this is every atom carrying `v_i`, as the paper writes it. Under `keys` a payload column after its atom's last join key is no level (a payload column before a join key stays one, by rule (iii)), so a position whose variable is such a payload column has an empty `I_join`. `enumerate` then skips to the next position instead of ending the branch, and the variable is bound in lines 16–19, from the leaf rows, as the paper binds every payload column (§3.2.4: worst-case optimal "on the set of join key values, before expanding the bag of tuples"). The skip is kermit's: the paper's attribute orders hold join keys only, so it has no such position.

## Invariants

- **Iterators in `I_join` move in lockstep.** `enumerate(i)` opens every iterator in `I_join` into its node for `v_i`, one level below where it stood, and the `up()` calls after the scan restore them. An atom descends only at its own variables, so its depth counts its variables bound so far, not the position: in the triangle below, T opens at X and at Z, never at Y. A failed `open()` leaves its iterator where it was (`HashTrieIterator::open`, as for `TrieIterator::open`), so when one fails the descent stops and only the iterators that opened go back up. Under `enumerate`'s contract only an empty root can fail, and then no row passes the position. Pinned by `enumerate_returns_every_iterator_to_its_pre_root`.
- **`enumerate` opens iterators on entry and ascends before returning.** The paper's iterators start in their root nodes and step `down` as soon as a hash matches (lines 10–11): into the node of their next variable or, after their last, into the tuple chain (Table 1: `tuples` is "only possible after calling down on the last trie level"). A `HashTrieIterator` has no position inside a chain: `open()` fails at the last level, and `leaf_tuples` reads the chain from the last level's bucket. So every iterator starts at its pre-root, and `enumerate(i)` opens its participants into their nodes for `v_i` on entry: each `down` is deferred until the variable it serves is reached. The scans, probes and leaf product are the paper's. Over a lazy `HashTrie` (`--ds-layout-expansion lazy`), an `open()` may build the child it enters; the algorithm is unchanged and sees the eager trie's keys and leaf chains, so a child a join never enters is never built.
- **Each atom is opened exactly `levels()` times** (#116). An atom participates at the positions of its levels' variables, in order, since its columns follow the plan; a position no trie has a level for is skipped, never ended (`enumerate`'s `participating.is_empty()` branch, which under `all` never runs, since every variable is some trie's level). So after the last position every iterator stands at its last level, or, for a trie with no level (an index with no join key), at its pre-root, where `leaf_tuples` lends its whole chain, possibly empty (an empty chain gives no candidates). Pinned by `payload_is_skipped_at_its_position_and_bound_from_the_leaf_rows` and `atoms_with_no_level_cross_their_whole_chains`.
- **`I_scan` choice is local to each level.** The argmin is recomputed on every call to `enumerate(i)`; the choice doesn't affect correctness, only the cost of probing the other iterators.
- **Hash collision rejection is at the leaves.** Inner-level `lookup(h)` may succeed on a false-positive hash match. The leaf-level `verify_and_construct` is the only place where actual key equality is enforced — every result tuple emerges from there. The `*_collision_*` tests in [`kermit-algos/src/hash/hash_triejoin.rs`](../../kermit-algos/src/hash/hash_triejoin.rs) force a real collision (a test-only `hash(k) = k mod 10` strategy on a real `HashTrie`) and check that the phantom candidate is rejected both by `verify_and_construct` directly and end-to-end through `join_for_each`.
- **Constants and repeated variables never reach the join.** `kermit::db::prepare` turns each body atom into an access (`kermit_algos::extract_accesses`): a constant is a filter on its column, and a repeated variable is an equality with its first occurrence. The build applies the filter while it copies the rows into the hash trie (`TupleView`), so every `HashTrieIterator` this algorithm sees is a real hash trie whose columns are all distinct variables, and `verify_and_construct` is the one site of the leaf check above. An atom of constants only builds no index: its row count multiplies the results on a structure that keeps duplicates (`Relation::KEEPS_DUPLICATES`, true for `HashTrie`), and a zero count ends the query.
- **Placeholders never reach the join.** `kermit::db::prepare` runs `rewrite_placeholders` ([placeholder_rewrite.rs](../../kermit-algos/src/placeholder_rewrite.rs)) first, so every `_` arrives as its own fresh variable and each atom carries one variable per column. `emit_leaf` relies on that, since it expects every iterator at its leaf once the last variable is bound, and panicked on any placeholder before the rewrite (issue #73). Regression: the `trailing_placeholder` and `middle_placeholder` patterns of the standard suite.
- **The result is streamed, not materialised.** `Probe` holds the caller's sink; `emit_leaf` re-borrows each leaf chain through `leaf_tuples()` as a `LeafRows` view over its relation's tuple buffer (#111, walked through the trie's links, #101 B2) instead of collecting it, and the cross-product cursors and output row are scratch allocated once per join, so emitting a tuple allocates nothing: line 19's `produce(t)` is `emit(&row)` (issue #65). A caller that needs the rows collects them with a sink that pushes into a flat `Tuples` batch.
- **The leaf product walks cursors, never indices** (#101 B2). A trie's leaf chain is threaded through its links, so it has no random access: `row(i)` would cost O(i). `LeafScratch` therefore holds one `LeafCursor` per relation, an index into a listed chain's ids (a pruned singleton) or the current row's id in a trie's chain, and `emit_leaf` runs Algorithm 3's lines 17–19 as an odometer over them:
  - if any relation's chain `is_empty()`, the product is empty and it returns;
  - otherwise every cursor starts at its chain's `first()`;
  - a candidate is the row `row_at(cursor[k])` of each relation k;
  - `advance_cursor` advances `cursor[0]`; when a chain's `advance` runs past its last row, that cursor is reset to `first()` and the carry moves to the next relation; the product ends when every digit has overflowed.

  So every combination is visited once, the first relation's cursor turning fastest, and the first relation's chain is walked again for each step of the others. The cursors are sized once in `LeafScratch::new`, so the walk allocates nothing per result row (`kermit/tests/result_allocation.rs`). Pinned by `emit_leaf_resets_each_cursor_on_carry` (three relations with chains of lengths 1, 2 and 3): without the reset on carry, the product reads past a chain's end and panics.
- **Variable ordering arrives as a plan.** `join_for_each` receives the plan inside an [`ExecutedQuery`](../../kermit-algos/src/executed.rs), produced ahead of time by a [`QueryOptimiser`](../optimisers/) and checked against the query when the type was built. The default [`LexicographicOptimiser`](../optimisers/lexicographic.md) plans the canonical order of the join keys, then of the payload (#116): every optimiser plans payload last, so a `keys` trie's levels are its index's first columns. The plan may bind an atom's columns in any order: [`plan_indexes`](../../kermit-algos/src/index_plan.rs) builds each atom's hash trie with its columns in plan order, so the term order of every atom HTJ sees equals its hash trie's column order, and nothing in this algorithm depends on how the relation is stored. LFTJ shares the exact same `QueryPlan` contract.

## Complexity

Let `n` be the number of query variables, `m` the number of body atoms, `H(I_j)` the set of hashed join-key tuples of the `j`-th relation, and `x` any fractional edge cover of the query hypergraph. The paper's Theorem 3.2, proved under set semantics:

> The time complexity of Algorithm 3 is in O(n · m · ∏ |H(I_j)|^x_j), and its space complexity is in O(n · m).

Under bag semantics the bound holds on the join-key values, before the leaf product expands each match into its bag of tuples (§3.2.4).

In terms of cost per call:

| Operation | Time | Notes |
|---|---|---|
| `enumerate(i)` per call | O(size(I_scan) · \|I_join\|) expected | one `lookup` per other participant for each hash of the scanned table; `lookup` is O(1) expected |
| `emit_leaf` | O(∏ chain_size · arity) | cross-product over leaf chains, verify each candidate in O(arity); each cursor step is one link load and one row read |
| `verify_and_construct` | O(arity + ∑ \|E_j\|) | bounded by the total number of predicate variables |

In the common case (no hash collisions, each chain has length 1), `emit_leaf` is O(arity) per result.

## Worked micro-example

Triangle query `Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).` with:

- R = {(1, 2)}
- S = {(2, 3), (2, 4)}
- T = {(1, 3), (1, 4)}

Variable ordering: `[X=0, Y=1, Z=2]`. `predicate_variables = [[0, 1], [1, 2], [0, 2]]`. `variable_to_iter_map = [[0, 2], [0, 1], [1, 2]]`.

Trace:

- `enumerate(0)`: X. `I_join = {R, T}`. `open` R and T from their pre-roots into their roots. Both hold one hash, so `I_scan` is R, the first minimum.
  - R's hash `h(1)`: T has it.
  - `enumerate(1)`: Y. `I_join = {R, S}`. `open` R into its child under X = 1, and S into its root. Both hold one hash; `I_scan` is R.
    - R's hash `h(2)`: S has it.
    - `enumerate(2)`: Z. `I_join = {S, T}`. `open` S into its child under Y = 2 and T into its child under X = 1; each holds `h(3)` and `h(4)`, and `I_scan` is S.
      - S's hash `h(3)`: T has it.
        - `enumerate(3)`, past the last variable: `emit_leaf`. Chains R [(1, 2)], S [(2, 3)], T [(1, 3)]; the one candidate verifies to [1, 2, 3]. **Emit.**
      - S's hash `h(4)`: T has it.
        - `emit_leaf`: chains R [(1, 2)], S [(2, 4)], T [(1, 4)]; [1, 2, 4]. **Emit.**
      - S's table is exhausted; `up` S and T.
    - R's table is exhausted; `up` R and S.
  - R's table is exhausted; `up` R and T, back to their pre-roots.

(`h(3)` and `h(4)` come in bucket order, which the hash decides.)

Output (sorted): `[[1, 2, 3], [1, 2, 4]]`.

Related test: `join_algo_triangle` in [`kermit-algos/src/hash/hash_triejoin.rs`](../../kermit-algos/src/hash/hash_triejoin.rs) exercises this same triangle-query path under `HashTriejoin` (with different sample data).

### With payload, under `--ds-layout-levels keys` (#116)

`Q(X, P, Y) :- R(X, P), S(X, Y), T(Y).` with R = {(1, 10), (1, 11), (2, 20)}, S = {(1, 5), (2, 6)} and T = {(5), (7)}. X and Y are join keys and P is payload, so under `keys` R is `[X, P]` with one level, S `[X, Y]` with two and T `[Y]` with one. `variable_to_iter_map` under the plan X, P, Y is `[[0, 1], [], [1, 2]]`:

- `enumerate(0)`: X. `I_join = {R, S}`. X = 1 is in both; `open` R and S.
  - `enumerate(1)`: P. `I_join` is empty: no trie has a level for P, so skip.
  - `enumerate(2)`: Y. `I_join = {S, T}`. S below X = 1 holds {5}, T holds {5, 7}: Y = 5. `open` S and T.
    - `enumerate(3)`: `emit_leaf`. R's chain at X = 1 holds (1, 10) and (1, 11), S's (1, 5), T's (5): two candidates, both verified, P bound from R's rows: [1, 10, 5] and [1, 11, 5]. **Emit both.**
- X = 2: S below X = 2 holds {6}, which T lacks.

Output: `[[1, 10, 5], [1, 11, 5]]`, as under `all`. The payload-last plan X, Y, P builds the same three tries and skips P last. Pinned by `payload_is_skipped_at_its_position_and_bound_from_the_leaf_rows`, under both plans and both policies.

## See also

- [`HashTrie`](../data-structures/hash-trie.md) — the only data structure this algorithm consumes.
- [`LeapfrogTriejoin`](./leapfrog-triejoin.md) — sibling worst-case-optimal algorithm using sorted (LFTJ) intersection.
- [`docs/optimisers/`](../optimisers/) — the `QueryOptimiser` implementations that plan the `QueryPlan` this algorithm executes.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; this algorithm must pass all 16 patterns under `HashTrie` (Priorities item 1).
