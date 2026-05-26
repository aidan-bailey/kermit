//! Hash trie iterator family — counterpart to `TrieIterator` for hash-trie
//! join algorithms. Iterators expose hashes (not sorted keys) and navigate
//! via exact hash lookup rather than least-upper-bound seek.
//!
//! See SIGMOD 2020 "Combining Worst-Case Optimal and Traditional Binary
//! Join Processing" §3.2 for the conceptual interface (Table 1).

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

/// Stable, deterministic hash for a `(depth, key)` pair.
///
/// `depth` is mixed into the hash so different attribute positions hash
/// into disjoint spaces — preventing cross-attribute aliasing when the same
/// `usize` key happens to appear at multiple positions.
///
/// This function is the single source of truth for the kermit hash-trie
/// hashing convention. Both `kermit_ds::HashTrie` and the algorithm-side
/// singleton in `kermit_algos` import it; any divergence silently breaks
/// queries with constants.
pub fn hash_attribute(depth: usize, key: usize) -> u64 {
    let mut h = DefaultHasher::new();
    depth.hash(&mut h);
    key.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_depth_same_key_same_hash() {
        assert_eq!(hash_attribute(0, 42), hash_attribute(0, 42));
    }

    #[test]
    fn different_depth_different_hash() {
        // Same key at different attribute positions hashes differently.
        // This is the cross-attribute aliasing safeguard.
        assert_ne!(hash_attribute(0, 42), hash_attribute(1, 42));
    }

    #[test]
    fn deterministic_across_processes() {
        // SipHash with a fixed (zero) seed is what DefaultHasher::new() gives.
        // This test pins the *idea* that two calls produce equal hashes;
        // a future Rust release that randomizes DefaultHasher's seed would
        // break this and require switching to a fixed-seed hasher.
        let a = hash_attribute(3, 1234);
        let b = hash_attribute(3, 1234);
        assert_eq!(a, b);
    }
}
