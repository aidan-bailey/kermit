//! Contract suites for `HashIndex`, the keyed-family structure — the
//! counterpart of `hash_trie_tests.rs`.
//!
//! `HashIndex` implements `KeyedIterable`, a third iterator family, so it
//! cannot be passed to `relation_trie_test_suite!` or `hash_trie_test_suite!`;
//! `keyed_index_test_suite!` (in `common/macros.rs`) states the keyed
//! contract: `probe` lends exactly a key's rows in source order, a key of no
//! columns lends every row, inserts keep that true through every rehash.
//! One invocation per hasher alias, matching `kermit/tests/join_tests.rs`,
//! plus one under a strategy that puts every row in one bucket.

use {
    kermit_ds::HashIndex,
    kermit_iters::{FxHashStrategy, HashStrategy, KeyedIterable, LayoutOption, SipHashStrategy},
};
mod common;

type HashIndexSip = HashIndex<SipHashStrategy>;
type HashIndexFx = HashIndex<FxHashStrategy>;

/// Test-only strategy that sends every value to hash 0: every row of an index
/// lands in bucket 0, so the key comparison alone must filter. A twin lives in
/// `kermit-ds/src/ds/hash_index/tests.rs`, where it is shown to leave every
/// row in one chain. The two are deliberately not shared: neither crate target
/// should ship a colliding strategy in its public API.
#[derive(Copy, Clone, Default, Debug)]
pub struct OneBucketHash;

impl LayoutOption for OneBucketHash {
    const NAME: &'static str = "one-bucket";
}

impl HashStrategy for OneBucketHash {
    fn hash(_key: usize) -> u64 { 0 }
}

type HashIndexOneBucket = HashIndex<OneBucketHash>;

#[test]
fn the_one_bucket_strategy_hashes_every_value_alike() {
    assert_eq!(OneBucketHash::hash(1), OneBucketHash::hash(2));
    assert_eq!(OneBucketHash::hash(0), OneBucketHash::hash(usize::MAX));
}

keyed_index_test_suite!(HashIndexSip);

keyed_index_test_suite!(HashIndexFx);

keyed_index_test_suite!(HashIndexOneBucket);

/// The rows of a loaded `HashIndex`, scanned: the file's rows in file order
/// (the fixtures write them ascending).
fn scanned_rows<H: HashStrategy>(relation: &HashIndex<H>) -> Vec<Vec<usize>> {
    relation.scan().map(<[usize]>::to_vec).collect()
}

// The Parquet round-trip: an index loaded from a file keys on every column
// (the header's level count is its arity) and scans the file's rows.
parquet_test_suite!(HashIndexSip, scanned_rows);

parquet_test_suite!(HashIndexFx, scanned_rows);
