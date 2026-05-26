//! Hash trie data structure (paper-faithful, "correctness only" scope).
//!
//! See `docs/data-structures/hash-trie.md` for the worked example, complexity
//! table, and invariants.

// The `hash_table` primitive lands in Phase 2 of the hash-trie-join build
// out. Its public surface is consumed by `HashTrie`/`HashTrieIter` in
// subsequent phases; until those land, every item appears unused to clippy.
#[allow(dead_code)]
mod hash_table;
