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

/// The home bucket of `hash` in a table of `2^log2_capacity` buckets: the
/// index [`HashTable::bucket_index`] computes, as a free function so a
/// [`BucketRun`] can compute it without its table. The two are pinned
/// together by `home_bucket_matches_bucket_index`; `bucket_index` keeps its
/// own body, so the serial path's code is unchanged.
pub(super) fn home_bucket(hash: u64, log2_capacity: u32) -> usize {
    let mixed = hash.wrapping_mul(MULTIPLIERS[log2_capacity as usize]);
    (mixed >> (64 - log2_capacity)) as usize
}

/// A probe that reached the end of its region (see [`BucketRun`]).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Overflow;

/// A contiguous run of a table's buckets, lent to one worker by
/// [`HashTable::with_runs`]. A run probes only within the
/// `region_buckets`-sized region that holds a key's home bucket, and never
/// grows: a probe that would cross the region's end is refused with
/// [`Overflow`], for the caller to insert afterwards with ordinary probing.
/// Regions, not runs, bound a probe, so which keys overflow does not depend
/// on how many runs the table is split into.
pub(super) struct BucketRun<'a, V> {
    /// The index, in the whole table, of `buckets[0]`.
    first: usize,
    /// Regions are this many buckets, aligned to multiples of it.
    region_buckets: usize,
    log2_capacity: u32,
    buckets: &'a mut [Option<Entry<V>>],
    /// Keys this run inserted; [`HashTable::with_runs`] adds them to `len`.
    inserted: &'a mut usize,
}

/// What [`BucketRun::entry`] found: the key's value, or the empty bucket
/// where the key would go.
pub(super) enum RunEntry<'r, V> {
    Occupied(&'r mut V),
    Vacant(VacantBucket<'r, V>),
}

/// An empty bucket inside a run, ready to take one key.
pub(super) struct VacantBucket<'r, V> {
    slot: &'r mut Option<Entry<V>>,
    hash: u64,
    inserted: &'r mut usize,
}

impl<'r, V> VacantBucket<'r, V> {
    /// Stores `value` under the probed hash and returns it.
    pub(super) fn insert(self, value: V) -> &'r mut V {
        *self.inserted += 1;
        &mut self
            .slot
            .insert(Entry {
                hash: self.hash,
                value,
            })
            .value
    }
}

impl<V> BucketRun<'_, V> {
    /// Probes for `hash` from its home bucket to the end of the home's
    /// region: the key's value if it is there, the first empty bucket if it
    /// is not, or [`Overflow`] if neither comes before the region's end.
    /// The run must hold the key's home bucket.
    pub(super) fn entry(&mut self, hash: u64) -> Result<RunEntry<'_, V>, Overflow> {
        let home = home_bucket(hash, self.log2_capacity);
        debug_assert!(
            (self.first..self.first + self.buckets.len()).contains(&home),
            "bucket {home} is outside this run"
        );
        let region_end = (home / self.region_buckets + 1) * self.region_buckets;
        let position = (home - self.first..region_end - self.first)
            .find(|&idx| {
                self.buckets[idx]
                    .as_ref()
                    .is_none_or(|entry| entry.hash == hash)
            })
            .ok_or(Overflow)?;
        Ok(match self.buckets[position] {
            | Some(_) => RunEntry::Occupied(
                &mut self.buckets[position]
                    .as_mut()
                    .expect("matched Some just above")
                    .value,
            ),
            | None => RunEntry::Vacant(VacantBucket {
                slot: &mut self.buckets[position],
                hash,
                inserted: &mut *self.inserted,
            }),
        })
    }
}

/// One contiguous run of a table that [`HashTable::map_in_runs`] is
/// mapping: its buckets, and the mapped table's buckets at the same
/// positions.
pub(super) struct MapRun<'a, V, W> {
    source: &'a mut [Option<Entry<V>>],
    target: &'a mut [Option<Entry<W>>],
}

impl<V, W> MapRun<'_, V, W> {
    /// Moves each value of the run, as `f(value)`, into the same bucket of
    /// the mapped table, in bucket order.
    pub(super) fn map(self, mut f: impl FnMut(V) -> W) {
        for (source, target) in self.source.iter_mut().zip(self.target.iter_mut()) {
            *target = source.take().map(
                |Entry {
                     hash,
                     value,
                 }| Entry {
                    hash,
                    value: f(value),
                },
            );
        }
    }
}

/// The log2 capacity a table starts at unless it is built at another: 4
/// buckets.
pub(crate) const INITIAL_LOG2_CAPACITY: u32 = 2;

/// The smallest log2 capacity the paper's sizing gives a table: 2 buckets,
/// for one tuple (`2^⌈log2(1.25)⌉`). Children sized from their lists start
/// here; the root keeps [`INITIAL_LOG2_CAPACITY`] (#88).
pub(crate) const PAPER_MIN_LOG2_CAPACITY: u32 = 1;

/// The smallest log2 capacity, at least `min_log2`, at which a table holds
/// `keys` keys without its load-factor cap firing: the smallest
/// `p ≥ min_log2` with `keys · den ≤ 2^p · num`.
///
/// [`HashTable::entry_or_insert_with`] grows when
/// `(len + 1) · den > capacity · num`, and `len + 1 ≤ keys` for every insert
/// up to the `keys`-th distinct hash. So a table built at this capacity
/// never grows while it receives at most `keys` distinct hashes. At an 80 %
/// cap this is the paper's `⌈log2(1.25·keys)⌉` (Algorithm 2, line 3), except
/// below `min_log2`.
///
/// # Panics
///
/// If the table would need `2^64` buckets or more, which no `Vec` can hold.
pub(crate) fn log2_capacity_for(keys: usize, load_factor: LoadFactor, min_log2: u32) -> u32 {
    // `u128`, so `keys · den` cannot overflow for any `usize`.
    let num = load_factor.numerator() as u128;
    let den = load_factor.denominator() as u128;
    let buckets_needed = (keys as u128 * den).div_ceil(num);
    let p = buckets_needed
        .next_power_of_two()
        .trailing_zeros()
        .max(min_log2);
    assert!(
        p < 64,
        "capacity overflow: {keys} keys at a {}% load factor need 2^{p} buckets",
        load_factor.numerator()
    );
    p
}

/// Open-addressing hash table.
///
/// Capacity is always `2^log2_capacity`. A table starts at 4 buckets
/// (`log2_capacity = 2`) unless built at another capacity with
/// [`with_log2_capacity`](Self::with_log2_capacity). The table grows by
/// doubling when occupancy
/// exceeds the configured load factor, 70 % by default.
pub(crate) struct HashTable<V> {
    log2_capacity: u32,
    len: usize,
    buckets: Vec<Option<Entry<V>>>,
}

impl<V> HashTable<V> {
    /// An empty table of 4 buckets.
    pub fn new() -> Self { Self::with_log2_capacity(INITIAL_LOG2_CAPACITY) }

    /// An empty table of `2^log2_capacity` buckets.
    ///
    /// # Panics
    ///
    /// Unless `1 ≤ log2_capacity < 64`: [`bucket_index`](Self::bucket_index)
    /// shifts by `64 - log2_capacity`, and `MULTIPLIERS` covers exponents
    /// below 64.
    pub fn with_log2_capacity(log2_capacity: u32) -> Self {
        assert!(
            (1..64).contains(&log2_capacity),
            "log2 capacity must be in 1..64, got {log2_capacity}"
        );
        let capacity = 1usize << log2_capacity;
        Self {
            log2_capacity,
            len: 0,
            buckets: (0..capacity).map(|_| None).collect(),
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

    /// The table with every value replaced by `f(value)`, called in bucket
    /// order: the same capacity, the same `len`, and every key in the bucket
    /// it occupied. Algorithm 2's "store `M_next` in `B`" (line 12) for a
    /// whole table: a table of tuple lists cannot hold the children built
    /// from them in place, since the two value types differ.
    pub(super) fn map<W>(self, mut f: impl FnMut(V) -> W) -> HashTable<W> {
        // Allocated, not collected: `collect` reuses the source allocation
        // when `Entry<W>` is no larger than `Entry<V>`, and
        // `shell_heap_bytes` would count the spare room.
        let mut buckets = Vec::with_capacity(self.buckets.len());
        buckets.extend(self.buckets.into_iter().map(|slot| {
            slot.map(
                |Entry {
                     hash,
                     value,
                 }| Entry {
                    hash,
                    value: f(value),
                },
            )
        }));
        HashTable {
            log2_capacity: self.log2_capacity,
            len: self.len,
            buckets,
        }
    }

    /// [`map`](Self::map) for a parallel caller: lends `map_runs` the table
    /// as `parts` contiguous [`MapRun`]s, one per worker, and returns the
    /// mapped table once `map_runs` has returned.
    ///
    /// # Panics
    ///
    /// Unless `parts` is a power of two no larger than the capacity, and if
    /// `map_runs` returns without mapping every run that holds a value,
    /// whose values would otherwise be lost.
    pub(super) fn map_in_runs<W>(
        mut self, parts: usize, map_runs: impl FnOnce(Vec<MapRun<'_, V, W>>),
    ) -> HashTable<W> {
        let capacity = self.buckets.len();
        assert!(
            parts.is_power_of_two() && parts <= capacity,
            "{parts} runs do not split {capacity} buckets"
        );
        let mut buckets: Vec<Option<Entry<W>>> = (0..capacity).map(|_| None).collect();
        let run_len = capacity / parts;
        map_runs(
            self.buckets
                .chunks_mut(run_len)
                .zip(buckets.chunks_mut(run_len))
                .map(|(source, target)| MapRun {
                    source,
                    target,
                })
                .collect(),
        );
        assert!(
            self.buckets.iter().all(Option::is_none),
            "map_in_runs: a run was left unmapped"
        );
        HashTable {
            log2_capacity: self.log2_capacity,
            len: self.len,
            buckets,
        }
    }

    /// Lends `fill` the bucket array as `parts` contiguous [`BucketRun`]s,
    /// each a whole number of `region_buckets`-sized regions (a region is
    /// the whole table when that is smaller), and adds the keys they insert
    /// to `len` when `fill` returns. The `presized:N` build hands one run
    /// to each worker; scoping the runs here keeps `len` right.
    ///
    /// # Panics
    ///
    /// Unless `parts` and `region_buckets` are powers of two and every run
    /// holds whole regions.
    pub(super) fn with_runs<R>(
        &mut self, parts: usize, region_buckets: usize,
        fill: impl FnOnce(Vec<BucketRun<'_, V>>) -> R,
    ) -> R {
        let capacity = self.buckets.len();
        assert!(parts.is_power_of_two() && region_buckets.is_power_of_two());
        let region_buckets = region_buckets.min(capacity);
        assert!(
            parts * region_buckets <= capacity,
            "{parts} runs of whole {region_buckets}-bucket regions do not fit {capacity} buckets"
        );
        let run_len = capacity / parts;
        let log2_capacity = self.log2_capacity;
        let mut inserted = vec![0usize; parts];
        let runs = self
            .buckets
            .chunks_mut(run_len)
            .zip(inserted.iter_mut())
            .enumerate()
            .map(|(k, (buckets, inserted))| BucketRun {
                first: k * run_len,
                region_buckets,
                log2_capacity,
                buckets,
                inserted,
            })
            .collect();
        let result = fill(runs);
        self.len += inserted.iter().sum::<usize>();
        result
    }
}

/// A hash whose home bucket in a table of `2^log2_capacity` buckets is
/// `home`. Distinct `low` values give distinct hashes with the same home.
/// The bucket index multiplies by an odd `MULTIPLIERS[p]`, so it is
/// inverted by multiplying by its inverse modulo `2^64`.
#[cfg(test)]
pub(super) fn hash_with_home(home: usize, log2_capacity: u32, low: u64) -> u64 {
    assert!(
        low >> (64 - log2_capacity) == 0,
        "low bits must stay below the index"
    );
    let m = MULTIPLIERS[log2_capacity as usize];
    // Newton's iteration for 1/m mod 2^64: m·m ≡ 1 (mod 8) for odd m, and
    // each step doubles the correct low bits (3 → 6 → 12 → 24 → 48 → 96).
    let mut inverse = m;
    for _ in 0..5 {
        inverse = inverse.wrapping_mul(2u64.wrapping_sub(m.wrapping_mul(inverse)));
    }
    debug_assert_eq!(m.wrapping_mul(inverse), 1);
    (((home as u64) << (64 - log2_capacity)) | low).wrapping_mul(inverse)
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
    #[cfg_attr(
        miri,
        ignore = "a cost test scanning ~10^6 hashes; nothing here is unsafe"
    )]
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
        for (i, hash) in [
            0x1000_0000_0000_0000_u64,
            0x5000_0000_0000_0000,
            0x9000_0000_0000_0000,
        ]
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

    #[test]
    fn with_log2_capacity_builds_that_many_empty_buckets() {
        for p in 1..=10 {
            let t: HashTable<u32> = HashTable::with_log2_capacity(p);
            assert_eq!(t.buckets_len(), 1 << p);
            assert_eq!(t.len(), 0);
            assert_eq!(t.log2_capacity, p);
            assert_eq!(
                t.shell_heap_bytes(),
                (1 << p) * std::mem::size_of::<Option<Entry<u32>>>()
            );
        }
        let new: HashTable<u32> = HashTable::new();
        assert_eq!(new.log2_capacity, INITIAL_LOG2_CAPACITY);
        assert_eq!(
            new.shell_heap_bytes(),
            HashTable::<u32>::with_log2_capacity(INITIAL_LOG2_CAPACITY).shell_heap_bytes()
        );
    }

    #[test]
    #[should_panic(expected = "log2 capacity must be in 1..64")]
    fn with_log2_capacity_rejects_zero() {
        let _: HashTable<u32> = HashTable::with_log2_capacity(0);
    }

    #[test]
    #[should_panic(expected = "log2 capacity must be in 1..64")]
    fn with_log2_capacity_rejects_sixty_four() {
        let _: HashTable<u32> = HashTable::with_log2_capacity(64);
    }

    /// `log2_capacity_for(keys, lf, INITIAL_LOG2_CAPACITY)` is the smallest
    /// log2 capacity of at least 2 whose table holds `keys` keys under the
    /// cap: sufficient, and one less would not be.
    #[test]
    fn log2_capacity_for_is_the_smallest_sufficient_capacity() {
        let most_keys = if cfg!(miri) {
            64
        } else {
            2_000
        };
        for percent in [1u8, 10, 25, 50, 70, 80, 95, 99] {
            let lf = LoadFactor::percent(percent).unwrap();
            for keys in 0..most_keys {
                let p = log2_capacity_for(keys, lf, INITIAL_LOG2_CAPACITY);
                let holds = |p: u32| keys * 100 <= (1usize << p) * usize::from(percent);
                assert!(
                    p >= INITIAL_LOG2_CAPACITY,
                    "{percent}%, {keys} keys: below 4 buckets"
                );
                assert!(holds(p), "{percent}%, {keys} keys: 2^{p} is too small");
                assert!(
                    p == INITIAL_LOG2_CAPACITY || !holds(p - 1),
                    "{percent}%, {keys} keys: 2^{p} is not the smallest"
                );
            }
        }
    }

    /// At an 80 % cap the capacity is the paper's `2^⌈log2(1.25·|L|)⌉`
    /// (Algorithm 2, line 3), apart from the 4-bucket floor.
    #[test]
    fn log2_capacity_for_at_eighty_percent_is_the_papers_sizing() {
        let lf = LoadFactor::percent(80).unwrap();
        for keys in 2..2_000usize {
            // ⌈log2(1.25·keys)⌉ = ⌈log2(⌈5·keys/4⌉)⌉, since 2^p is a whole
            // number; for keys ≥ 2 it is at least 2, so the floor is moot.
            let paper = (5 * keys).div_ceil(4).next_power_of_two().trailing_zeros();
            assert_eq!(
                log2_capacity_for(keys, lf, INITIAL_LOG2_CAPACITY),
                paper,
                "{keys} keys"
            );
        }
        // The paper allocates 2 buckets for one tuple; the root never goes
        // below 4 buckets; children sized from their lists may
        // (`PAPER_MIN_LOG2_CAPACITY`).
        assert_eq!(
            log2_capacity_for(1, lf, INITIAL_LOG2_CAPACITY),
            INITIAL_LOG2_CAPACITY
        );
    }

    /// With the paper's 2-bucket minimum, the capacity at an 80 % cap is the
    /// paper's `2^⌈log2(1.25·|L|)⌉` for every list length, one tuple
    /// included.
    #[test]
    fn log2_capacity_for_with_the_papers_minimum_is_the_papers_sizing() {
        let lf = LoadFactor::percent(80).unwrap();
        for keys in 1..2_000usize {
            let paper = (5 * keys).div_ceil(4).next_power_of_two().trailing_zeros();
            assert_eq!(
                log2_capacity_for(keys, lf, PAPER_MIN_LOG2_CAPACITY),
                paper,
                "{keys} keys"
            );
        }
    }

    #[test]
    #[should_panic(expected = "capacity overflow")]
    fn log2_capacity_for_rejects_an_unallocatable_capacity() {
        log2_capacity_for(
            usize::MAX,
            LoadFactor::percent(1).unwrap(),
            INITIAL_LOG2_CAPACITY,
        );
    }

    /// A table built at `log2_capacity_for(keys, lf, min_log2)` takes `keys`
    /// distinct inserts without growing, at every load factor and either
    /// floor: the guarantee the root-capacity (#88) and child-capacity (#107)
    /// Configs rest on.
    #[test]
    fn a_presized_table_never_grows_for_its_keys() {
        let most_keys = if cfg!(miri) {
            100
        } else {
            1_000
        };
        for percent in [1u8, 10, 25, 50, 70, 95, 99] {
            let lf = LoadFactor::percent(percent).unwrap();
            for min_log2 in [PAPER_MIN_LOG2_CAPACITY, INITIAL_LOG2_CAPACITY] {
                for keys in [0, 1, 2, 3, 7, most_keys] {
                    let p = log2_capacity_for(keys, lf, min_log2);
                    let mut t: HashTable<usize> = HashTable::with_log2_capacity(p);
                    // A full-period LCG, so every hash is distinct.
                    let mut hash: u64 = 1;
                    for k in 0..keys {
                        t.entry_or_insert_with(hash, lf, || k);
                        assert_eq!(
                            t.buckets_len(),
                            1 << p,
                            "{percent}%, at least 2^{min_log2}: grew on key {k} of {keys}"
                        );
                        hash = hash.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
                    }
                    assert_eq!(t.len(), keys);
                }
            }
        }
    }

    /// A table of `2^log2` buckets holding nothing yet, for the run tests.
    fn empty(log2: u32) -> HashTable<u32> { HashTable::with_log2_capacity(log2) }

    /// Occupied bucket indices, in order.
    fn occupied<V>(t: &HashTable<V>) -> Vec<usize> {
        (0..t.buckets_len())
            .filter(|&i| t.hash_at(i).is_some())
            .collect()
    }

    /// Σ over occupied buckets of how far each key sits past its home.
    fn total_displacement<V>(t: &HashTable<V>) -> usize {
        let cap = t.buckets_len();
        let log2 = cap.trailing_zeros();
        (0..cap)
            .filter_map(|i| t.hash_at(i).map(|h| (i + cap - home_bucket(h, log2)) % cap))
            .sum()
    }

    #[test]
    fn home_bucket_matches_bucket_index() {
        let mut lcg = crate::test_support::Lcg(0x88);
        for log2 in 1..20 {
            let table = empty(log2);
            for _ in 0..200 {
                let hash = lcg.next_usize() as u64;
                assert_eq!(
                    home_bucket(hash, log2),
                    table.bucket_index(hash),
                    "2^{log2}"
                );
            }
        }
    }

    #[test]
    fn hash_with_home_lands_where_it_says() {
        for log2 in [2u32, 5, 12] {
            for home in [0usize, 1, (1 << log2) - 1] {
                for low in [0u64, 1, 7] {
                    assert_eq!(home_bucket(hash_with_home(home, log2, low), log2), home);
                }
            }
        }
    }

    /// Two runs of one 8-bucket region each in a 16-bucket table: a run
    /// finds a key it already holds, inserts at the first empty bucket, and
    /// counts its inserts into `len`.
    #[test]
    fn a_run_finds_a_key_or_inserts_at_the_first_empty_bucket() {
        let (a, b) = (hash_with_home(2, 4, 1), hash_with_home(2, 4, 2));
        let mut table = empty(4);
        table.with_runs(2, 8, |mut runs| {
            let run = &mut runs[0];
            let RunEntry::Vacant(slot) = run.entry(a).unwrap() else {
                panic!("a is new")
            };
            slot.insert(10);
            let RunEntry::Vacant(slot) = run.entry(b).unwrap() else {
                panic!("b is new")
            };
            slot.insert(20);
            let RunEntry::Occupied(value) = run.entry(a).unwrap() else {
                panic!("a is held")
            };
            assert_eq!(*value, 10);
        });
        assert_eq!(table.len(), 2);
        assert_eq!(table.index_of(a), Some(2));
        assert_eq!(table.index_of(b), Some(3));
        assert_eq!(table.get(b), Some(&20));
    }

    /// A probe that reaches its region's end is refused, at the end of a
    /// middle region and at the table's last bucket alike: a run never
    /// wraps and never spills into the next region.
    #[test]
    fn a_run_overflows_at_its_region_end_and_never_wraps() {
        let mut table = empty(4);
        table.with_runs(2, 8, |mut runs| {
            for (run, home) in [(0, 7), (1, 15)] {
                let first = hash_with_home(home, 4, 1);
                let RunEntry::Vacant(slot) = runs[run].entry(first).unwrap() else {
                    panic!("{home}: first key is new")
                };
                slot.insert(1);
                let second = hash_with_home(home, 4, 2);
                assert!(
                    runs[run].entry(second).is_err(),
                    "{home}: second key overflows"
                );
            }
        });
        assert_eq!(table.len(), 2);
        assert_eq!(occupied(&table), vec![7, 15]);
    }

    /// Filling by runs, then inserting the overflow by ordinary probing,
    /// occupies exactly the buckets that inserting every key one at a time
    /// occupies, with the same total displacement. That is linear probing's
    /// order independence (Knuth, TAOCP §6.4), and what the `presized:N`
    /// build relies on.
    #[test]
    fn runs_and_tail_fill_what_sequential_insertion_fills() {
        let lf = LoadFactor::percent(70).unwrap();
        let seeds = if cfg!(miri) {
            2
        } else {
            40
        };
        let mut overflowed = 0;
        for seed in 0..seeds {
            let mut lcg = crate::test_support::Lcg(seed);
            let keys: Vec<u64> = (0..85).map(|_| lcg.next_usize() as u64).collect();
            let mut sequential = empty(7);
            for &k in &keys {
                sequential.entry_or_insert_with(k, lf, || 0);
            }
            let mut by_runs = empty(7);
            let tail: Vec<u64> = by_runs.with_runs(4, 16, |mut runs| {
                let mut tail = Vec::new();
                for &k in &keys {
                    let run = &mut runs[home_bucket(k, 7) / 32];
                    match run.entry(k) {
                        | Ok(RunEntry::Vacant(slot)) => {
                            slot.insert(0);
                        },
                        | Ok(RunEntry::Occupied(_)) => {},
                        | Err(Overflow) => tail.push(k),
                    }
                }
                tail
            });
            overflowed += tail.len();
            for k in tail {
                by_runs.entry_or_insert_with(k, lf, || 0);
            }
            assert_eq!(by_runs.len(), sequential.len(), "seed {seed}");
            assert_eq!(occupied(&by_runs), occupied(&sequential), "seed {seed}");
            assert_eq!(
                total_displacement(&by_runs),
                total_displacement(&sequential),
                "seed {seed}"
            );
            for &k in &keys {
                assert!(by_runs.index_of(k).is_some(), "seed {seed}: {k:#x} lost");
            }
        }
        assert!(
            overflowed > 0,
            "no seed overflowed a region; the test proves nothing"
        );
    }

    /// A table of 40 hashes (two of which share a home bucket at 4
    /// buckets), each valued by the order it was inserted in.
    fn filled() -> HashTable<usize> {
        let mut t = HashTable::new();
        let (first, second) = colliding_pair(&t);
        t.entry_or_insert_with(first, LoadFactor::default(), || 0);
        t.entry_or_insert_with(second, LoadFactor::default(), || 1);
        let mut hash: u64 = 1;
        for k in 2..40 {
            t.entry_or_insert_with(hash, LoadFactor::default(), || k);
            hash = hash.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        }
        assert_eq!(t.len(), 40, "every hash distinct");
        t
    }

    /// `map` keeps the table's shape and every key in its bucket, and
    /// replaces each value.
    #[test]
    fn map_keeps_every_key_in_its_bucket() {
        let t = filled();
        let (cap, len) = (t.buckets_len(), t.len());
        let before: Vec<(Option<u64>, Option<usize>)> = (0..cap)
            .map(|i| (t.hash_at(i), t.value_at(i).copied()))
            .collect();
        let mapped: HashTable<String> = t.map(|v| format!("v{v}"));
        assert_eq!(mapped.buckets_len(), cap);
        assert_eq!(mapped.len(), len);
        assert_eq!(
            mapped.shell_heap_bytes(),
            cap * std::mem::size_of::<Option<Entry<String>>>()
        );
        for (i, (hash, value)) in before.into_iter().enumerate() {
            assert_eq!(mapped.hash_at(i), hash, "bucket {i}");
            assert_eq!(
                mapped.value_at(i).cloned(),
                value.map(|v| format!("v{v}")),
                "bucket {i}"
            );
        }
    }

    /// `map` calls `f` in bucket order, the order in which Algorithm 2's
    /// line 9 visits the populated buckets.
    #[test]
    fn map_calls_f_in_bucket_order() {
        let t = filled();
        let order: Vec<usize> = t.iter().map(|(_, &v)| v).collect();
        let mut seen = Vec::new();
        let _ = t.map(|v| seen.push(v));
        assert_eq!(seen, order);
    }

    /// Mapping in runs builds what `map` builds, however the table is cut.
    #[test]
    fn map_in_runs_matches_map() {
        let serial = filled().map(|v| v * 10);
        for parts in [1, 2, 4, 8, serial.buckets_len()] {
            let in_runs = filled().map_in_runs(parts, |runs| {
                assert_eq!(runs.len(), parts);
                for run in runs {
                    run.map(|v| v * 10);
                }
            });
            assert_eq!(in_runs.buckets_len(), serial.buckets_len(), "{parts} runs");
            assert_eq!(in_runs.len(), serial.len(), "{parts} runs");
            assert_eq!(
                in_runs.shell_heap_bytes(),
                serial.shell_heap_bytes(),
                "{parts} runs"
            );
            for i in 0..serial.buckets_len() {
                assert_eq!(
                    in_runs.hash_at(i),
                    serial.hash_at(i),
                    "{parts} runs, bucket {i}"
                );
                assert_eq!(
                    in_runs.value_at(i),
                    serial.value_at(i),
                    "{parts} runs, bucket {i}"
                );
            }
        }
    }

    /// `map` allocates exactly its buckets even when the new values are
    /// smaller, where `collect` would reuse, and over-count, the source's
    /// allocation.
    #[test]
    fn map_to_a_smaller_value_allocates_exactly_its_buckets() {
        let t = filled();
        let cap = t.buckets_len();
        let mapped: HashTable<()> = t.map(|_| ());
        assert_eq!(
            mapped.shell_heap_bytes(),
            cap * std::mem::size_of::<Option<Entry<()>>>()
        );
    }

    /// A run left unmapped would lose its values, so `map_in_runs` refuses.
    #[test]
    #[should_panic(expected = "a run was left unmapped")]
    fn map_in_runs_refuses_to_lose_a_run() {
        let _ = filled().map_in_runs(2, |mut runs: Vec<MapRun<'_, usize, usize>>| {
            runs.pop().expect("two runs").map(|v| v);
        });
    }
}
