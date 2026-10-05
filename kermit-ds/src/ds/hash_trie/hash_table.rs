//! Open-addressing hash table with linear probing, keyed on raw `u64`
//! hashes.
//!
//! Paper-faithful implementation of §3.3.1 of "Combining Worst-Case Optimal
//! and Traditional Binary Join Processing" (SIGMOD 2020), with one
//! departure. Capacity is a power of two, `2^p`; collisions are resolved by
//! linear probing within the bucket array. The bucket index is the high `p`
//! bits of the hash, as in the paper, but of the hash multiplied by a
//! constant that changes with the capacity — the departure; see
//! `HashTable::bucket_index` for why.
//!
//! Internal to the `hash_trie` module. Not exposed outside the crate.

use super::config::LoadFactor;

/// The bucket-index multiplier for each capacity exponent: a table with
/// `2^p` buckets multiplies by `MULTIPLIERS[p]` (see
/// [`HashTable::bucket_index`]). Each is odd, so the multiply is a bijection
/// on `u64` that discards none of the hash, and each is a separate SplitMix64
/// output, so the multipliers of different capacities are unrelated.
const MULTIPLIERS: [u64; 64] = {
    let mut multipliers = [0; 64];
    let mut p = 0;
    while p < 64 {
        multipliers[p] = splitmix64(p as u64) | 1;
        p += 1;
    }
    multipliers
};

/// The `index`-th output of SplitMix64 seeded with 0 (Steele, Lea & Flood,
/// "Fast Splittable Pseudorandom Number Generators", OOPSLA 2014).
const fn splitmix64(index: u64) -> u64 {
    let mut z = index.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

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

    /// Bucket index for `hash` at the current capacity `2^p`: the high `p`
    /// bits of `hash × MULTIPLIERS[p]` — multiplicative hashing (Knuth,
    /// TAOCP vol. 3, §6.4) with a multiplier that changes with the capacity.
    ///
    /// The paper indexes by the high bits of the hash itself. That index is
    /// a prefix of the index at every larger capacity, so iterating a table,
    /// which walks its buckets in order, yields its keys sorted by the index
    /// of every smaller capacity too. A table rebuilt in that order — as
    /// when a `HashTrie` is rebuilt from its own tuples — starts small and
    /// doubles, and at each smaller capacity the keys it holds so far all
    /// share its lowest buckets: linear probing piles them into one cluster
    /// and the build turns quadratic (issue #66). With a multiplier per
    /// capacity the index at one capacity says nothing about the index at
    /// another, so iteration order no longer predicts where a key lands
    /// while the rebuilt table is smaller than its source. Once the two are
    /// the same size the keys do arrive in bucket order, but no denser than
    /// the source held them — at most its load-factor cap — so the extra
    /// cost stays bounded instead of growing with the keys.
    ///
    /// One order remains that no index computed from the hash and the
    /// capacity alone can defuse: the iteration orders of two or more large
    /// tables of the same capacity, concatenated. Their keys share that
    /// capacity's multiplier, so their densities add up in the low buckets.
    /// Only a seed that differs per table instance would cover it.
    ///
    /// The price is a table load and a multiply per probe sequence, and no
    /// space. A salt fixed per trie depth would not do: the source and the
    /// rebuilt table would share it, and the salted hash would cluster just
    /// the same.
    fn bucket_index(&self, hash: u64) -> usize {
        let p = self.log2_capacity;
        let mixed = hash.wrapping_mul(MULTIPLIERS[p as usize]);
        (mixed >> (64 - p)) as usize
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

    /// Consumes the table, returning its bucket array: `buckets_len()`
    /// slots in bucket order, each `None` or the entry stored there. Used
    /// by the radix build to move each scratch entry out exactly once, by
    /// the position `index_of` reported for it.
    pub fn into_buckets(self) -> Vec<Option<Entry<V>>> { self.buckets }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        kermit_iters::{FxHashStrategy, HashStrategy, SipHashStrategy},
    };

    #[test]
    fn new_starts_empty_with_4_buckets() {
        let t: HashTable<u32> = HashTable::new();
        assert_eq!(t.len(), 0);
        assert_eq!(t.buckets.len(), 4);
        assert_eq!(t.log2_capacity, 2);
    }

    /// Odd, so multiplying by one is a bijection on `u64` and discards none
    /// of the hash; distinct, so no two capacities index alike.
    #[test]
    fn multipliers_are_odd_and_distinct() {
        for (p, m) in MULTIPLIERS.iter().enumerate() {
            assert_eq!(m % 2, 1, "the multiplier for 2^{p} buckets is even");
            assert!(
                !MULTIPLIERS[..p].contains(m),
                "the multiplier for 2^{p} buckets repeats"
            );
        }
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

    /// Two distinct hashes whose home is the same bucket of `t`.
    fn colliding_pair<V>(t: &HashTable<V>) -> (u64, u64) {
        let first = 0x4000_0000_0000_0000;
        let second = (first + 1..)
            .find(|&h| t.bucket_index(h) == t.bucket_index(first))
            .unwrap();
        (first, second)
    }

    #[test]
    fn get_probes_past_collision() {
        let mut t: HashTable<u32> = HashTable::new();
        // Force a probe: seat a different hash in the target's home bucket
        // and the target in the next one.
        let (other, target) = colliding_pair(&t);
        let home = t.bucket_index(target);
        let next = (home + 1) % t.buckets_len();
        t.buckets[home] = Some(Entry {
            hash: other,
            value: 1,
        });
        t.buckets[next] = Some(Entry {
            hash: target,
            value: 2,
        });
        t.len = 2;
        assert_eq!(t.get(target), Some(&2));
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
        let (first, second) = colliding_pair(&t);
        *t.entry_or_insert_with(first, LoadFactor::default(), || 1) = 1;
        *t.entry_or_insert_with(second, LoadFactor::default(), || 2) = 2;
        assert_ne!(
            t.index_of(second),
            Some(t.bucket_index(second)),
            "the second entry should have been probed past its home bucket"
        );
        assert_eq!(t.get(first), Some(&1));
        assert_eq!(t.get(second), Some(&2));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn resize_triggers_above_load_factor() {
        let mut t: HashTable<u32> = HashTable::new();
        // Capacity 4, threshold > 4 * 0.7 = 2.8. The 3rd insert should resize.
        let hashes = [
            0x0000_0000_0000_0000,
            0x4000_0000_0000_0000,
            0x8000_0000_0000_0000,
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

    /// Keys per table in the build-cost tests below: enough for a clustered
    /// rebuild to cost many times an unrelated one, few enough for Miri.
    const BUILD_KEYS: usize = if cfg!(miri) {
        1 << 9
    } else {
        1 << 12
    };

    /// A table mapping `H::hash(key)` to `key` for each of `keys`, inserted
    /// in the given order.
    fn build<H: HashStrategy>(keys: impl Iterator<Item = usize>) -> HashTable<usize> {
        let mut t = HashTable::new();
        for key in keys {
            t.entry_or_insert_with(H::hash(key), LoadFactor::default(), || key);
        }
        t
    }

    /// Probe steps a fresh table takes to absorb `hashes` in the given
    /// order. Linear probing never moves an entry until the next doubling,
    /// so an insert's probe sequence is its distance from its home bucket
    /// plus one, read off right after the insert; a doubling re-probes
    /// every entry.
    fn build_probes(hashes: &[u64]) -> usize {
        let mut t: HashTable<()> = HashTable::new();
        let mut probes = 0;
        for &hash in hashes {
            let capacity = t.buckets_len();
            t.entry_or_insert_with(hash, LoadFactor::default(), || ());
            let cap = t.buckets_len();
            let probe = |h: u64| (t.index_of(h).unwrap() + cap - t.bucket_index(h)) % cap + 1;
            probes += if cap == capacity {
                probe(hash)
            } else {
                t.iter().map(|(h, _)| probe(h)).sum()
            };
        }
        probes
    }

    /// Asserts that absorbing `hashes` in `iteration_order` costs at most
    /// twice what absorbing them in `key_order` does.
    fn assert_no_dearer(label: &str, iteration_order: &[u64], key_order: &[u64]) {
        let (rebuilt, baseline) = (build_probes(iteration_order), build_probes(key_order));
        assert!(
            rebuilt <= 2 * baseline,
            "{label}: rebuilding in iteration order took {rebuilt} probes, key order {baseline}"
        );
    }

    /// Rebuilding a table in another table's iteration order costs no more
    /// than building it in an order unrelated to its layout (issue #66).
    ///
    /// The finished table cannot show the difference — under linear probing
    /// the total displacement of a key set does not depend on insertion
    /// order — so this counts the probes the build took. With the hash's
    /// own high bits as the bucket index, the iteration-order build costs
    /// many times more, and the factor grows linearly with the keys.
    #[test]
    fn rebuilding_in_iteration_order_costs_no_more_than_key_order() {
        fn check<H: HashStrategy>() {
            let source = build::<H>(0..BUILD_KEYS);
            let iteration_order: Vec<u64> = source.iter().map(|(h, _)| h).collect();
            let key_order: Vec<u64> = (0..BUILD_KEYS).map(H::hash).collect();
            assert_no_dearer(H::NAME, &iteration_order, &key_order);
        }
        check::<SipHashStrategy>();
        check::<FxHashStrategy>();
    }

    /// The same holds when the source table is larger than the rebuilt one,
    /// as when a projection or a filtered rebuild drops keys: the rebuilt
    /// table then never reaches the source's final capacity, so the source's
    /// order must not predict the index at any smaller one.
    #[test]
    fn rebuilding_a_subset_in_a_larger_tables_iteration_order_costs_no_more_than_key_order() {
        fn check<H: HashStrategy>() {
            let source = build::<H>(0..4 * BUILD_KEYS);
            let iteration_order: Vec<u64> = source
                .iter()
                .filter(|&(_, &key)| key % 4 == 0)
                .map(|(h, _)| h)
                .collect();
            let key_order: Vec<u64> = (0..BUILD_KEYS).map(|k| H::hash(4 * k)).collect();
            assert_no_dearer(H::NAME, &iteration_order, &key_order);
        }
        check::<SipHashStrategy>();
        check::<FxHashStrategy>();
    }

    /// The radix build (issue #91) fills each partition's scratch root with
    /// keys that share the top `bits` of their hash — a clustered input, as
    /// in #66. The per-capacity multiplier must keep absorbing one partition
    /// no dearer than absorbing an unrestricted key set of the same size.
    /// Without it, `bits` shared index bits would put every key of a small
    /// table in one bucket.
    #[test]
    #[cfg_attr(miri, ignore = "a cost test scanning ~10^6 hashes; nothing here is unsafe")]
    fn absorbing_one_radix_partition_costs_no_more_than_unrestricted_keys() {
        fn check<H: HashStrategy>() {
            for bits in [1u32, 4, 8] {
                let partition: Vec<u64> = (0..)
                    .map(H::hash)
                    .filter(|hash| hash >> (64 - bits) == 0)
                    .take(BUILD_KEYS)
                    .collect();
                let unrestricted: Vec<u64> = (0..BUILD_KEYS).map(H::hash).collect();
                let (clustered, baseline) = (build_probes(&partition), build_probes(&unrestricted));
                assert!(
                    clustered <= 2 * baseline,
                    "{} radix:{bits}: one partition took {clustered} probes, unrestricted keys \
                     {baseline}",
                    H::NAME
                );
            }
        }
        check::<SipHashStrategy>();
        check::<FxHashStrategy>();
    }

    #[test]
    fn into_buckets_returns_every_entry_at_its_bucket() {
        let mut t: HashTable<u32> = HashTable::new();
        for (i, hash) in [0x1000_0000_0000_0000_u64, 0x5000_0000_0000_0000, 0x9000_0000_0000_0000]
            .into_iter()
            .enumerate()
        {
            t.entry_or_insert_with(hash, LoadFactor::default(), || i as u32);
        }
        let positions: Vec<(usize, u64)> = t
            .iter()
            .map(|(hash, _)| (t.index_of(hash).unwrap(), hash))
            .collect();
        let capacity = t.buckets_len();
        let buckets = t.into_buckets();
        assert_eq!(buckets.len(), capacity);
        assert_eq!(buckets.iter().flatten().count(), 3);
        for (idx, hash) in positions {
            assert_eq!(buckets[idx].as_ref().map(|e| e.hash), Some(hash));
        }
    }
}
