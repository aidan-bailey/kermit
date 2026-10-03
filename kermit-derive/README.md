# kermit-derive

Procedural macros for the Kermit workspace. Currently ships a single derive:

## `#[derive(IntoTrieIter)]`

Generates an `IntoIterator` impl for a trie-iterator struct, wrapping it in `kermit_iters::TrieIteratorWrapper` so the trie can be consumed with a plain `for` loop that yields `Vec<usize>` tuples.

Requirements on the annotated struct:

- Must implement `kermit_iters::TrieIterator` (which extends `kermit_iters::LinearIterator`).
- Must take the lifetime `'a` as its first generic parameter, optionally followed by type parameters (a Layout such as a seek strategy, as on `TreeTrieIter<'a, S>`). Their bounds, inline or in a `where` clause, are carried into the generated impl. A second lifetime or a const parameter is rejected.

A runnable example lives in [`tests/derive_into_trie_iter.rs`](tests/derive_into_trie_iter.rs). Production uses are in `kermit-ds` (`TreeTrieIter`, `ColumnTrieIter`).
