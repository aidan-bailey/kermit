# kermit-algos

Join algorithms and query optimisers for the Kermit workspace. Implements two worst-case-optimal multi-way joins: the [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481), over any data structure implementing [`TrieIterable`](../kermit-iters/src/trie.rs), and the Hash Trie Join (SIGMOD 2020), over any implementing [`HashTrieIterable`](../kermit-iters/src/hash_trie.rs).

## Layout

`src/` is split by **iterator family**. The sorted and hash families share no code and never reference each other — their iterator contracts cannot merge (least-upper-bound `seek` needs sorted data; hash `lookup` is exact-match), and that fork propagates through the whole stack. See [`ARCHITECTURE.md`](../ARCHITECTURE.md) § "Two Iterator Families".

```
src/
  lib.rs            flat public facade + the JoinAlgorithm registry enum
  join_algo.rs      the JoinAlgo<DS> trait every algorithm implements
  analysis.rs       canonical variable numbering, shared by planners and executors
  placeholder_rewrite.rs
                    turns each _ into its own fresh variable
  access.rs         extracts each body atom's access: its relation, its constant and
                    repeated-variable filter, and the columns it keeps
  index_plan.rs     turns a plan into the indexes a query builds (relation, filter,
                    column order)
  optimiser/        planning: QueryOptimiser, QueryPlan, CatalogStats, the policies
  sorted/           the TrieIterable family
  hash/             the HashTrieIterable family
```

Each family directory holds a `mod.rs` documenting the family contract and one module per algorithm, named to match its `docs/algorithms/<name>.md` page and its CLI value. There are no view or wrapper types: constants and repeated variables are filters on the index a query builds (see `access.rs`), so an executor only ever reads real indexes.

## Public API

Everything is re-exported flat from the crate root — consumers write `kermit_algos::HashTriejoin`, never the family path. The two `*Iter` types (`LeapfrogTriejoinIter`, `LeapfrogJoinIter`) and the traits driving them are `pub(crate)`: they are implementation detail, exercised by the inline tests, and not reachable from outside the crate.

- [`JoinAlgo`](src/join_algo.rs) — the algorithm trait. Takes a `QueryPlan`, a parsed `JoinQuery`, and a map from predicate name to data structure reference; its one required method, `join_for_each`, passes each result tuple to a caller-supplied sink as a borrowed slice, so neither algorithm materialises its result. A caller that needs the rows collects them with a sink that pushes into a flat `Tuples` batch.
- [`LeapfrogTriejoin`](src/sorted/leapfrog_triejoin.rs) — the sorted-family entry point (`-a leapfrog-triejoin`).
- [`HashTriejoin`](src/hash/hash_triejoin.rs) — the hash-family entry point (`-a hash-triejoin`).
- [`QueryOptimiser`](src/optimiser/mod.rs) — plans the variable ordering (`QueryPlan`) that `JoinAlgo::join_for_each` executes. Implementations: `LexicographicOptimiser` (default), `CardinalityOptimiser`, `CostBasedOptimiser`.
- [`extract_accesses`](src/access.rs) and [`plan_indexes`](src/index_plan.rs) — the two steps the caller runs around planning. `extract_accesses` turns each body atom into an `Access` (a constant or a repeated variable becomes a filter, not a predicate); `plan_indexes` turns the plan into the `IndexSpec`s (relation, filter, column order) the query builds, and renames the atoms to those indexes, with their variables in index-column order. A `JoinAlgo` impl therefore receives atoms whose term order equals their index's column order, and needs no handling of constants, repeated variables or reordered copies.
- `JoinAlgorithm` / `Optimiser` — CLI registry enums used by the binary to pick an implementation at runtime.

## Relationship to other crates

Depends on [`kermit-iters`](../kermit-iters) for iterator traits and [`kermit-parser`](../kermit-parser) for query ASTs — nothing else. [`kermit-ds`](../kermit-ds) is a **dev-dependency only**, so there is no production edge to the data structures: `LeapfrogTriejoin` accepts any `TrieIterable` (in practice `TreeTrie` and `ColumnTrie`), `HashTriejoin` any `HashTrieIterable` (in practice `HashTrie`).

See [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the overall query-evaluation flow, and the per-component docs under [`docs/algorithms/`](../docs/algorithms) and [`docs/optimisers/`](../docs/optimisers).

## Extending

[`CLAUDE.md`](../CLAUDE.md) holds the authoritative checklists ("Adding a new join algorithm", "Adding a new query optimiser"). In outline, a new algorithm is:

1. `src/<family>/<name>.rs`, implementing [`JoinAlgo<DS>`](src/join_algo.rs) with `DS` narrowed to the family you traverse. An algorithm in a *new* family also needs that family's `mod.rs` documenting the family contract.
2. Register it in the family's `mod.rs`, re-export it from `src/lib.rs`, and add a `JoinAlgorithm` variant there.
3. Wire the CLI in `kermit/src/main.rs` and `kermit/src/execution.rs`.
4. Add a `define_multiway_join_test_suite!` invocation per index structure in `kermit/tests/join_tests.rs`, under both optimisers, and a cell in `kermit/tests/all_orders.rs`.
5. Write `docs/algorithms/<name>.md` from the template.
