# `<AlgorithmName>`

> **Status:** [stable | experimental] · **CLI:** `-a <algorithm-name>` (or "internal helper — not CLI-exposed") · **Implementation:** [`kermit_algos::<module>`](../../kermit-algos/src/<file>.rs)

## What it is

One short paragraph. What the algorithm does, its complexity class (worst-case-optimal, output-sensitive, etc.), and a citation to the paper if there is one.

## Pseudocode

```
function name(args):
    line-by-line pseudocode that mirrors the literature
```

Map each pseudocode block to a Rust function in the implementation so a reader can navigate from paper to source.

## Invariants

- One bullet per invariant the algorithm relies on. Include the *consequence* — what breaks if the invariant breaks. The reader needs to know whether a candidate change is safe.

## Complexity

| Operation | Time | Notes |
|---|---|---|
| `op_1` | O(…) | |
| `op_2` | O(…) | |

If there is a global bound (AGM, output-sensitive in the smallest input, etc.), state it above the table.

## Worked micro-example

A small example — three relations, ≤ four tuples each — that a reader can step through by hand. Show the state of the algorithm at each step. Link to a corresponding test case if one exists.

## See also

- Cross-references to companion docs in `docs/algorithms/` or `docs/data-structures/`.
- `define_multiway_join_test_suite!` ([`kermit/tests/common/macros.rs`](../../kermit/tests/common/macros.rs)) — combinatorial coverage; this algorithm must pass all 11 patterns under every compatible index structure (Priorities item 1).
