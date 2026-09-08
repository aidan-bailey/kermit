use {
    kermit_ds::{define_config_provider, ColumnTrie, Configured, HashTrie, HashTrieConfig, TreeTrie},
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

// The same round-trip under the one Config flag: pruned singleton levels must
// still yield every stored tuple.
define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig {
    singleton_pruning: true,
});

type HashTrieSipPruned = Configured<HashTrieSip, PruningOn>;
type HashTrieFxPruned = Configured<HashTrieFx, PruningOn>;

fn sorted_tuples_pruned<H: kermit_iters::HashStrategy>(
    relation: &Configured<HashTrie<H>, PruningOn>,
) -> Vec<Vec<usize>> {
    // `Configured` derefs to the inner `HashTrie`, so the inherent
    // `collect_tuples` is reachable unchanged.
    sorted_tuples(relation)
}

parquet_test_suite!(HashTrieSipPruned, sorted_tuples_pruned);

parquet_test_suite!(HashTrieFxPruned, sorted_tuples_pruned);
