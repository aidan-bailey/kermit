# kermit-algos

Join algorithms for the Kermit workspace. Implements two worst-case-optimal multi-way joins: the [Leapfrog Triejoin](https://arxiv.org/abs/1210.0481), generic over any data structure that implements [`TrieIterable`](../kermit-iters/src/trie.rs), and the Hash Trie Join, generic over any data structure that implements [`HashTrieIterable`](../kermit-iters/src/hash_trie.rs).

## Entry points

- [`LeapfrogTriejoin`](src/leapfrog_triejoin.rs) — the algorithm's [`JoinAlgo`](src/join_algo.rs) implementation. Takes a parsed [`JoinQuery`](../kermit-parser/src/join_query.rs) plus a map from predicate name to data structure reference, and returns an iterator over the join output.
- [`LeapfrogTriejoinIter`](src/leapfrog_triejoin.rs) — the lower-level iterator produced by the algorithm. Exposes `triejoin_open` / `triejoin_up` for manual driving in tests.
- [`LeapfrogJoinIter`](src/leapfrog_join.rs) — the inner intersection that powers each depth of the triejoin.
- [`HashTriejoin`](src/hash_triejoin.rs) — the Hash Trie Join's [`JoinAlgo`](src/join_algo.rs) implementation, over `HashTrieIterable` structures.
- `JoinAlgorithm` — CLI enum used by the binary to pick an algorithm at runtime (variants `LeapfrogTriejoin` and `HashTriejoin`).

## Relationship to other crates

Depends on [`kermit-iters`](../kermit-iters) for iterator traits and [`kermit-parser`](../kermit-parser) for query ASTs. `LeapfrogTriejoin` accepts any relation implementing `TrieIterable` — in practice `TreeTrie` and `ColumnTrie` from [`kermit-ds`](../kermit-ds); `HashTriejoin` accepts any relation implementing `HashTrieIterable` — in practice `HashTrie` from the same crate.

See [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the overall query-evaluation flow and [`ARCHITECTURE.md#leapfrog-triejoin`](../ARCHITECTURE.md#leapfrog-triejoin) for a walkthrough of the algorithm as implemented here.

## Extending

To add a new algorithm (see [`CLAUDE.md`](../CLAUDE.md) "Adding a new join algorithm" for the authoritative checklist):

1. Implement [`JoinAlgo<DS>`](src/join_algo.rs) on a marker type.
2. Add a variant to [`JoinAlgorithm`](src/lib.rs), extend its `FromStr` impl, and wire it up in `kermit/src/main.rs` (the `JoinAlgorithmSelector` enum and its `expand()`).
3. Add a `define_multiway_join_test_suite!` invocation per index structure in `kermit/tests/join_tests.rs` so the standard join patterns run against it.
