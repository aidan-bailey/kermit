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

use super::config::LoadFactor;

/// A bucket entry stores the full 64-bit hash (for collision disambiguation
/// during linear probing) and the value.
pub(crate) struct Entry<V> {
    pub hash: u64,
    pub value: V,
}

/// Open-addressing hash table.
///
/// Capacity is always `2^log2_capacity`. The initial capacity is 4
/// (`log2_capacity = 2`). The table grows by doubling when occupancy
/// exceeds the configured load factor, 70 % by default.
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

    /// Look up a value by exact hash. Returns `None` if not found.
    pub fn get(&self, hash: u64) -> Option<&V> {
        let mut idx = self.bucket_index(hash);
        let cap = self.buckets.len();
        for _ in 0..cap {
            match &self.buckets[idx] {
                | None => return None,
                | Some(entry) if entry.hash == hash => return Some(&entry.value),
                | Some(_) => idx = (idx + 1) % cap,
            }
        }
        None
    }

    /// Look up a mutable value by exact hash. Returns `None` if not found.
    pub fn get_mut(&mut self, hash: u64) -> Option<&mut V> {
        let mut idx = self.bucket_index(hash);
        let cap = self.buckets.len();
        for _ in 0..cap {
            match &self.buckets[idx] {
                | None => return None,
                | Some(entry) if entry.hash == hash => {
                    return self.value_at_mut(idx);
                },
                | Some(_) => idx = (idx + 1) % cap,
            }
        }
        None
    }

    /// Insert at `hash`, or return a `&mut V` to the existing entry. The
    /// `default` closure is invoked only if the slot is currently empty.
    ///
    /// Resizes the table when the occupancy would exceed `load_factor`
    /// (see [`Self::grow`]). The cap is a parameter rather than a field so
    /// that varying it costs no space per table.
    pub fn entry_or_insert_with<F: FnOnce() -> V>(
        &mut self, hash: u64, load_factor: LoadFactor, default: F,
    ) -> &mut V {
        let cap = self.buckets.len();
        let start = self.bucket_index(hash);
        let mut idx = start;
        // Pre-resize probe: this loop must both detect an *existing* entry
        // (return it, no insert) and locate the first empty slot. It is
        // bounded by `cap` for safety, though the load-factor cap guarantees
        // an empty bucket exists. Breaking on `None` leaves `idx` at that
        // empty slot.
        for _ in 0..cap {
            match &self.buckets[idx] {
                | Some(entry) if entry.hash == hash => {
                    return self.value_at_mut(idx).unwrap();
                },
                | Some(_) => idx = (idx + 1) % cap,
                | None => break,
            }
        }
        // About to insert a new entry. Check the cap first:
        //   LF > p/100  ⇔  (len + 1) / capacity > p / 100
        //              ⇔  (len + 1) * 100 > capacity * p
        if (self.len + 1) * load_factor.denominator() > cap * load_factor.numerator() {
            self.grow();
            let cap = self.buckets.len();
            let mut idx = self.bucket_index(hash);
            // Post-resize probe: reaching here proves the hash is absent (the
            // loop above broke on `None`), so this only needs to find an empty
            // slot — no equality check. Unbounded because the just-doubled
            // table certainly has room.
            loop {
                match &self.buckets[idx] {
                    | None => {
                        self.buckets[idx] = Some(Entry {
                            hash,
                            value: default(),
                        });
                        self.len += 1;
                        return self.value_at_mut(idx).unwrap();
                    },
                    | Some(_) => idx = (idx + 1) % cap,
                }
            }
        }
        self.buckets[idx] = Some(Entry {
            hash,
            value: default(),
        });
        self.len += 1;
        self.value_at_mut(idx).unwrap()
    }

    /// Double capacity and rehash all entries. Called by
    /// `entry_or_insert_with` when occupancy would exceed the caller's
    /// load-factor cap.
    fn grow(&mut self) {
        self.log2_capacity += 1;
        let new_cap = 1usize << self.log2_capacity;
        let old_buckets =
            std::mem::replace(&mut self.buckets, (0..new_cap).map(|_| None).collect());
        self.len = 0;
        for entry in old_buckets.into_iter().flatten() {
            self.insert_during_grow(entry);
        }
    }

    fn insert_during_grow(&mut self, entry: Entry<V>) {
        let cap = self.buckets.len();
        let mut idx = self.bucket_index(entry.hash);
        loop {
            match &self.buckets[idx] {
                | None => {
                    self.buckets[idx] = Some(entry);
                    self.len += 1;
                    return;
                },
                | Some(_) => idx = (idx + 1) % cap,
            }
        }
    }

    /// Iterate `(hash, &value)` over occupied buckets, in bucket-array order.
    ///
    /// Order depends on the hash values and the current capacity; it is not
    /// insertion order. Used by `HashTrieIter::next` to walk a node's
    /// occupied buckets in deterministic-per-trie order.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &V)> {
        self.buckets
            .iter()
            .filter_map(|slot| slot.as_ref().map(|e| (e.hash, &e.value)))
    }

    /// Bucket-array capacity (always a power of two).
    pub fn buckets_len(&self) -> usize { self.buckets.len() }

    /// Find the first occupied bucket index >= `start`. Returns
    /// `buckets_len()` if no occupied bucket exists at or after `start`.
    pub fn next_occupied(&self, start: usize) -> usize {
        // `get` rather than a `[start..]` slice so an out-of-range `start`
        // returns `buckets_len()` like the empty-range case, instead of
        // panicking.
        self.buckets
            .get(start..)
            .and_then(|tail| tail.iter().position(Option::is_some))
            .map_or(self.buckets.len(), |offset| start + offset)
    }

    /// Reference to the value at bucket `idx`, or `None` if empty.
    pub fn value_at(&self, idx: usize) -> Option<&V> {
        self.buckets
            .get(idx)
            .and_then(|slot| slot.as_ref().map(|e| &e.value))
    }

    /// Mutable reference to the value at bucket `idx`, or `None` if empty.
    fn value_at_mut(&mut self, idx: usize) -> Option<&mut V> {
        self.buckets[idx].as_mut().map(|e| &mut e.value)
    }

    /// Hash at bucket `idx`, or `None` if empty.
    pub fn hash_at(&self, idx: usize) -> Option<u64> {
        self.buckets
            .get(idx)
            .and_then(|slot| slot.as_ref().map(|e| e.hash))
    }

    /// Index of the bucket containing `hash`, or `None` if not present.
    pub fn index_of(&self, hash: u64) -> Option<usize> {
        let cap = self.buckets.len();
        let mut idx = self.bucket_index(hash);
        for _ in 0..cap {
            match &self.buckets[idx] {
                | None => return None,
                | Some(entry) if entry.hash == hash => return Some(idx),
                | Some(_) => idx = (idx + 1) % cap,
            }
        }
        None
    }

    /// Bytes allocated by this table's internal `Vec`, excluding the contained
    /// values (the caller is responsible for accumulating those).
    pub fn shell_heap_bytes(&self) -> usize {
        self.buckets.capacity() * std::mem::size_of::<Option<Entry<V>>>()
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

    #[test]
    fn get_returns_none_on_empty_table() {
        let t: HashTable<u32> = HashTable::new();
        assert!(t.get(0).is_none());
        assert!(t.get(u64::MAX).is_none());
    }

    #[test]
    fn get_returns_some_after_direct_insert() {
        let mut t: HashTable<u32> = HashTable::new();
        // Directly seat an entry for the get/probing test (entry_or_insert_with
        // comes in the next task).
        let idx = t.bucket_index(0x4000_0000_0000_0000);
        t.buckets[idx] = Some(Entry {
            hash: 0x4000_0000_0000_0000,
            value: 99,
        });
        t.len = 1;
        assert_eq!(t.get(0x4000_0000_0000_0000), Some(&99));
        assert!(t.get(0x4000_0000_0000_0001).is_none());
    }

    #[test]
    fn get_probes_past_collision() {
        let mut t: HashTable<u32> = HashTable::new();
        // Force a probe: put a different hash in slot 1 and the target in slot
        // 2.
        t.buckets[1] = Some(Entry {
            hash: 0x4000_0000_0000_0000,
            value: 1,
        });
        t.buckets[2] = Some(Entry {
            hash: 0x4000_0000_0000_0001,
            value: 2,
        });
        t.len = 2;
        // Both hash to bucket 1 (top 2 bits = 01). Linear probing finds
        // the target at slot 2.
        assert_eq!(t.get(0x4000_0000_0000_0001), Some(&2));
    }

    #[test]
    fn entry_inserts_new_value() {
        let mut t: HashTable<u32> = HashTable::new();
        *t.entry_or_insert_with(0x4000_0000_0000_0000, LoadFactor::default(), || 99) = 99;
        assert_eq!(t.get(0x4000_0000_0000_0000), Some(&99));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn entry_returns_existing_value() {
        let mut t: HashTable<u32> = HashTable::new();
        *t.entry_or_insert_with(0x4000_0000_0000_0000, LoadFactor::default(), || 99) = 99;
        let mut called = false;
        let _ = t.entry_or_insert_with(0x4000_0000_0000_0000, LoadFactor::default(), || {
            called = true;
            0
        });
        assert!(!called, "default closure called for an existing entry");
        assert_eq!(t.get(0x4000_0000_0000_0000), Some(&99));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn entry_handles_probe_collision() {
        let mut t: HashTable<u32> = HashTable::new();
        *t.entry_or_insert_with(0x4000_0000_0000_0000, LoadFactor::default(), || 1) = 1;
        *t.entry_or_insert_with(0x4000_0000_0000_0001, LoadFactor::default(), || 2) = 2;
        assert_eq!(t.get(0x4000_0000_0000_0000), Some(&1));
        assert_eq!(t.get(0x4000_0000_0000_0001), Some(&2));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn resize_triggers_above_load_factor() {
        let mut t: HashTable<u32> = HashTable::new();
        // Capacity 4, threshold > 4 * 0.7 = 2.8. The 3rd insert should resize.
        // Use hashes guaranteed to land in distinct buckets so we can observe
        // capacity changes via `buckets.len()` instead of through probing.
        let hashes = [
            0x0000_0000_0000_0000, // bucket 0
            0x4000_0000_0000_0000, // bucket 1
            0x8000_0000_0000_0000, // bucket 2
        ];
        for (i, &h) in hashes.iter().enumerate() {
            *t.entry_or_insert_with(h, LoadFactor::default(), || i as u32) = i as u32;
        }
        assert_eq!(t.len(), 3);
        // After 3 inserts (LF would be 3/4 = 0.75 without resize), the table
        // doubled to 8.
        assert_eq!(t.buckets.len(), 8);
        assert_eq!(t.log2_capacity, 3);
        // All entries still reachable.
        for &h in &hashes {
            assert!(t.get(h).is_some(), "hash {h:#x} lost across resize");
        }
    }

    #[test]
    fn resize_preserves_values() {
        let mut t: HashTable<String> = HashTable::new();
        let inputs = [
            (0x1000_0000_0000_0000, "a".to_string()),
            (0x5000_0000_0000_0000, "b".to_string()),
            (0x9000_0000_0000_0000, "c".to_string()),
            (0xD000_0000_0000_0000, "d".to_string()),
            (0xF000_0000_0000_0000, "e".to_string()),
        ];
        for (h, v) in &inputs {
            *t.entry_or_insert_with(*h, LoadFactor::default(), || v.clone()) = v.clone();
        }
        for (h, v) in &inputs {
            assert_eq!(t.get(*h), Some(v));
        }
    }

    #[test]
    fn iter_yields_all_occupied_entries() {
        let mut t: HashTable<u32> = HashTable::new();
        let inputs = [
            (0x1000_0000_0000_0000_u64, 1u32),
            (0x5000_0000_0000_0000_u64, 2u32),
            (0x9000_0000_0000_0000_u64, 3u32),
        ];
        for (h, v) in &inputs {
            *t.entry_or_insert_with(*h, LoadFactor::default(), || *v) = *v;
        }
        let mut collected: Vec<(u64, u32)> = t.iter().map(|(h, v)| (h, *v)).collect();
        collected.sort_by_key(|&(h, _)| h);
        assert_eq!(collected, vec![
            (0x1000_0000_0000_0000_u64, 1),
            (0x5000_0000_0000_0000_u64, 2),
            (0x9000_0000_0000_0000_u64, 3),
        ]);
    }

    #[test]
    fn iter_empty_table_yields_nothing() {
        let t: HashTable<u32> = HashTable::new();
        assert_eq!(t.iter().count(), 0);
    }

    /// With cap `p`%, the table doubles exactly on the insert that would
    /// push `(len + 1) / capacity` above `p / 100`.
    #[test]
    fn resizes_exactly_at_the_configured_cap() {
        for percent in [50u8, 70, 90] {
            let lf = LoadFactor::percent(percent).unwrap();
            let mut t: HashTable<usize> = HashTable::new();
            let mut last_cap = t.buckets_len();
            let mut hash: u64 = 1;
            for _ in 0..64 {
                let before_len = t.len();
                t.entry_or_insert_with(hash, lf, || 0);
                if t.buckets_len() != last_cap {
                    // Doubled on this insert: the *previous* occupancy plus one
                    // must have exceeded the cap for the old capacity.
                    assert!(
                        (before_len + 1) * 100 > last_cap * usize::from(percent),
                        "{percent}%: grew early at len {before_len}, cap {last_cap}"
                    );
                    last_cap = t.buckets_len();
                } else {
                    assert!(
                        (before_len + 1) * 100 <= last_cap * usize::from(percent),
                        "{percent}%: should have grown at len {before_len}, cap {last_cap}"
                    );
                }
                hash = hash.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
            }
        }
    }
}
