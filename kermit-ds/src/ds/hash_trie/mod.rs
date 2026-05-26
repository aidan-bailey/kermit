//! Hash trie data structure (paper-faithful, "correctness only" scope).
//!
//! See `docs/data-structures/hash-trie.md` for the worked example, complexity
//! table, and invariants.

// `hash_table`, `node`, and `implementation` are consumed crate-internally
// across Phase 3; `HashTrie` itself only becomes reachable from outside the
// `hash_trie` module once Phase 5 wires it into `ds/mod.rs`. Until then,
// rustc reports every item as dead. Gate the modules to keep CI's
// `-Dwarnings` happy in the interim.
#[allow(dead_code)]
mod hash_table;
#[allow(dead_code)]
mod implementation;
#[allow(dead_code)]
mod node;

#[allow(unused_imports)]
pub use implementation::HashTrie;
