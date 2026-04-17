# kermit-derive

Procedural macros for the Kermit workspace. Currently ships a single derive:

## `#[derive(IntoTrieIter)]`

Generates an `IntoIterator` impl for a trie-iterator struct, wrapping it in `kermit_iters::TrieIteratorWrapper` so the trie can be consumed with a plain `for` loop that yields `Vec<usize>` tuples.

Requirements on the annotated struct:

- Must implement `kermit_iters::TrieIterator` (which extends `kermit_iters::LinearIterator`).
- Must have a single lifetime parameter named `'a`.

A runnable example lives in [`tests/derive_into_trie_iter.rs`](tests/derive_into_trie_iter.rs). Production uses are in `kermit-ds` (`TreeTrieIter`, `ColumnTrieIter`).
