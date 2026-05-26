//! Open-addressing hash table with linear probing, keyed on raw `u64`
//! hashes.
//!
//! Paper-faithful implementation of §3.3.1 of "Combining Worst-Case Optimal
//! and Traditional Binary Join Processing" (SIGMOD 2020). Capacity is a
//! power of two; the bucket index is computed as `hash >> (64 - p)` where
//! capacity = 2^p; collisions are resolved by linear probing within the
//! bucket array.
//!
//! Internal to the `hash_trie` module. Not exposed outside the crate.

/// A bucket entry stores the full 64-bit hash (for collision disambiguation
/// during linear probing) and the value.
pub(crate) struct Entry<V> {
    pub hash: u64,
    pub value: V,
}

/// Open-addressing hash table.
///
/// Capacity is always `2^log2_capacity`. The initial capacity is 4
/// (`log2_capacity = 2`). The table grows by doubling when load factor
/// exceeds 0.7.
pub(crate) struct HashTable<V> {
    log2_capacity: u32,
    len: usize,
    buckets: Vec<Option<Entry<V>>>,
}

impl<V> HashTable<V> {
    /// Initial capacity 4.
    pub fn new() -> Self {
        Self {
            log2_capacity: 2,
            len: 0,
            buckets: (0..4).map(|_| None).collect(),
        }
    }

    /// Number of occupied buckets.
    pub fn len(&self) -> usize { self.len }

    /// Bucket index for a given hash: the high `log2_capacity` bits.
    fn bucket_index(&self, hash: u64) -> usize {
        let shift = 64 - self.log2_capacity;
        (hash >> shift) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_empty_with_4_buckets() {
        let t: HashTable<u32> = HashTable::new();
        assert_eq!(t.len(), 0);
        assert_eq!(t.buckets.len(), 4);
        assert_eq!(t.log2_capacity, 2);
    }

    #[test]
    fn bucket_index_uses_high_bits() {
        let t: HashTable<u32> = HashTable::new();
        // Capacity 4 => shift 62. Top 2 bits of the hash become the bucket.
        assert_eq!(t.bucket_index(0x0000_0000_0000_0000), 0);
        assert_eq!(t.bucket_index(0x4000_0000_0000_0000), 1);
        assert_eq!(t.bucket_index(0x8000_0000_0000_0000), 2);
        assert_eq!(t.bucket_index(0xC000_0000_0000_0000), 3);
        // Anything below the top 2 bits collapses to the same bucket.
        assert_eq!(t.bucket_index(0x0000_0000_FFFF_FFFF), 0);
    }
}
