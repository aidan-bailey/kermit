//! Hash trie data structure (paper-faithful, "correctness only" scope).
//!
//! See `docs/data-structures/hash-trie.md` for the worked example, complexity
//! table, and invariants.

// `hash_table::HashTable::get_mut` is reserved infrastructure consumed only by
// the table's own tests right now (`lookup` uses `index_of` instead); the
// algorithm-side code added in later phases is expected to use it. (`get` is
// now live: singleton pruning probes buckets with it.) Keep the dead-code gate
// on `hash_table` until that wiring lands.
mod build_mode;
mod config;
mod expansion;
#[allow(dead_code)]
mod hash_table;
mod hash_trie_iter;
#[cfg(test)]
mod identity;
mod implementation;
mod node;
mod parallel;
mod pruning;
mod radix;

#[cfg(any(test, feature = "test-hooks"))]
pub(crate) use parallel::take_parallel_builds;
#[cfg(feature = "test-hooks")]
pub use parallel::ParallelBuild;
pub use {
    build_mode::{HashTrieBuildMode, InvalidRadixBits, ParseHashTrieBuildModeError, RadixBits},
    config::{HashTrieConfig, InvalidLoadFactor, LoadFactor, ParseRootCapacityError, RootCapacity},
    expansion::{EagerExpansion, ExpansionPolicy, LazyExpansion},
    implementation::HashTrie,
    pruning::{NoPruning, PruningPolicy, SingletonPruning},
};
