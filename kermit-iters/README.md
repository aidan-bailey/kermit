# kermit-iters

Core iterator traits for the Kermit workspace. This crate has zero internal dependencies and sits at the root of the dependency graph — every other crate builds on the abstractions defined here.

## What lives here

- [`LinearIterator`](src/linear.rs) — sorted sequential iteration over a flat level. Methods: `key`, `next`, `seek`, `at_end`.
- [`TrieIterator`](src/trie.rs) — extends `LinearIterator` with `open` (descend to child level) and `up` (ascend to parent). The fundamental abstraction consumed by Leapfrog Triejoin.
- [`LinearIterable`](src/linear.rs) / [`TrieIterable`](src/trie.rs) — traits implemented by data structures that can produce such iterators.
- [`TrieIteratorWrapper`](src/trie.rs) — adapts any `TrieIterator` into a standard `Iterator<Item = Vec<usize>>` via depth-first traversal, optionally filtered by arity.
- [`JoinIterable`](src/joinable.rs) — marker trait that both iterator traits extend; unifies types consumable by `kermit-algos`.
- [`Key`](src/key_type.rs) — canonical `usize` key alias used throughout the workspace.

## Design rationale

See [`ARCHITECTURE.md`](../ARCHITECTURE.md) for the full trait hierarchy discussion. In short: decoupling the iterator traits from any specific data structure lets algorithms in `kermit-algos` work over any compatible trie implementation (`TreeTrie`, `ColumnTrie`, or a future addition) without modification.
