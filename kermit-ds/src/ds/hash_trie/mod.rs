//! Hash trie data structure (paper-faithful, "correctness only" scope).
//!
//! See `docs/data-structures/hash-trie.md` for the worked example, complexity
//! table, and invariants.

// `HashTrie` is reachable from outside this module only once Phase 5 wires
// it into `ds/mod.rs`. Until then, the type and its impls are dead-code from
// the lint's perspective, so we gate `implementation` and the `pub use`
// re-export until that wiring lands. `hash_table::HashTable::{get, get_mut}`
// are reserved for Phase 4's `HashTrieIter::lookup`, so keep the same gate
// on `hash_table` for now.
#[allow(dead_code)]
mod hash_table;
#[allow(dead_code)]
mod implementation;
mod node;

#[allow(unused_imports)]
pub use implementation::HashTrie;
