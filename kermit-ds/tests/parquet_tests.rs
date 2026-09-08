use {
    kermit_ds::{ColumnTrie, HashTrie, TreeTrie},
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};
mod common;

parquet_test_suite!(TreeTrie);

parquet_test_suite!(ColumnTrie);

// `HashTrie` has no tuple-shaped iterator (it is `HashTrieIterable`, not
// `TrieIterable`), so the round-trip is checked through `collect_tuples()`,
// whose order is hash-dependent and therefore sorted before comparison. One
// invocation per Layout alias, matching `kermit/tests/join_tests.rs`.
type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;

fn sorted_tuples<H: kermit_iters::HashStrategy>(relation: &HashTrie<H>) -> Vec<Vec<usize>> {
    let mut tuples = relation.collect_tuples();
    tuples.sort();
    tuples
}

parquet_test_suite!(HashTrieSip, sorted_tuples);

parquet_test_suite!(HashTrieFx, sorted_tuples);
