//! Layout option: which hash function `HashTrie` uses to convert tuple
//! attribute values (`usize`) into the `u64` hashes used by the algorithm.
//!
//! A `HashStrategy` is a zero-sized marker type passed as a generic
//! parameter to `HashTrie<H>` and used by the algorithm's singleton
//! constructor (via precomputed hash). Each implementation is also a
//! [`LayoutOption`] — the `NAME` constant flows into bench-report axes
//! under `ds_layout_hasher`.

use crate::optimization::LayoutOption;

/// A hash function for `usize` keys, selected at compile time.
pub trait HashStrategy: LayoutOption + Copy + Default + 'static {
    /// Hash a `usize` key value to a `u64`.
    fn hash(key: usize) -> u64;
}

/// SipHash via the standard library's `DefaultHasher`. The default for
/// `HashTrie<H>` — preserves pre-standard behavior.
#[derive(Copy, Clone, Default, Debug)]
pub struct SipHashStrategy;

impl LayoutOption for SipHashStrategy {
    const NAME: &'static str = "sip";
}

impl HashStrategy for SipHashStrategy {
    fn hash(key: usize) -> u64 {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };
        let mut h = DefaultHasher::new();
        key.hash(&mut h);
        h.finish()
    }
}

/// FxHash via the `rustc-hash` crate. Fast non-cryptographic hash;
/// typically ~10× faster per call than SipHash on small integer keys.
#[derive(Copy, Clone, Default, Debug)]
pub struct FxHashStrategy;

impl LayoutOption for FxHashStrategy {
    const NAME: &'static str = "fxhash";
}

impl HashStrategy for FxHashStrategy {
    fn hash(key: usize) -> u64 {
        use {
            rustc_hash::FxHasher,
            std::hash::{Hash, Hasher},
        };
        let mut h = FxHasher::default();
        key.hash(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sip_strategy_name_is_sip() {
        assert_eq!(<SipHashStrategy as LayoutOption>::NAME, "sip");
    }

    #[test]
    fn fxhash_strategy_name_is_fxhash() {
        assert_eq!(<FxHashStrategy as LayoutOption>::NAME, "fxhash");
    }

    #[test]
    fn sip_strategy_is_deterministic() {
        assert_eq!(SipHashStrategy::hash(42), SipHashStrategy::hash(42));
    }

    #[test]
    fn fxhash_strategy_is_deterministic() {
        assert_eq!(FxHashStrategy::hash(42), FxHashStrategy::hash(42));
    }

    #[test]
    fn sip_and_fxhash_produce_different_hashes() {
        // Different hash functions almost always produce different output for
        // the same input. Pin this to catch accidental aliasing (e.g., if
        // someone refactored one strategy to forward to the other).
        assert_ne!(SipHashStrategy::hash(42), FxHashStrategy::hash(42));
    }
}
