# kermit-algos

Join algorithms for the Kermit workspace. Currently implements the [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481) — a worst-case-optimal multi-way join — generic over any data structure that implements [`TrieIterable`](../kermit-iters/src/trie.rs).

## Entry points

- [`LeapfrogTriejoin`](src/leapfrog_triejoin.rs) — the algorithm's [`JoinAlgo`](src/join_algo.rs) implementation. Takes a parsed [`JoinQuery`](../kermit-parser/src/join_query.rs) plus a map from predicate name to data structure reference, and returns an iterator over the join output.
- [`LeapfrogTriejoinIter`](src/leapfrog_triejoin.rs) — the lower-level iterator produced by the algorithm. Exposes `triejoin_open` / `triejoin_up` for manual driving in tests.
- [`LeapfrogJoinIter`](src/leapfrog_join.rs) — the inner intersection that powers each depth of the triejoin.
- `JoinAlgorithm` — CLI enum used by the binary to pick an algorithm at runtime.

## Relationship to other crates

Depends on [`kermit-iters`](../kermit-iters) for iterator traits and [`kermit-parser`](../kermit-parser) for query ASTs. Accepts any relation implementing `TrieIterable` — in practice `TreeTrie` and `ColumnTrie` from [`kermit-ds`](../kermit-ds).

See [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the overall query-evaluation flow and [`ARCHITECTURE.md#leapfrog-triejoin`](../ARCHITECTURE.md#leapfrog-triejoin) for a walkthrough of the algorithm as implemented here.

## Extending

To add a new algorithm:

1. Implement [`JoinAlgo<DS>`](src/join_algo.rs) on a marker type.
2. Add a variant to [`JoinAlgorithm`](src/lib.rs) and wire it up in `kermit/src/main.rs`.
