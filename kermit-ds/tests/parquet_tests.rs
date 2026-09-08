use {
    kermit_ds::{
        define_config_provider, ColumnTrie, Configured, HashTrie, HashTrieConfig, LoadFactor,
        PruningPolicy, SingletonPruning, TreeTrie,
    },
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

fn sorted_tuples<H: kermit_iters::HashStrategy, P: PruningPolicy>(
    relation: &HashTrie<H, P>,
) -> Vec<Vec<usize>> {
    let mut tuples = relation.collect_tuples();
    tuples.sort();
    tuples
}

parquet_test_suite!(HashTrieSip, sorted_tuples);

parquet_test_suite!(HashTrieFx, sorted_tuples);

// The same round-trip under the pruning Layout: pruned singleton levels must
// still yield every stored tuple.
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;

parquet_test_suite!(HashTrieSipPruned, sorted_tuples);

parquet_test_suite!(HashTrieFxPruned, sorted_tuples);

// …and under the Config axis: a dense load factor keeps the round-trip whole.
define_config_provider!(NinetyPercent, HashTrieConfig, HashTrieConfig {
    load_factor: LoadFactor::percent(90).unwrap(),
});

type HashTrieSipDense = Configured<HashTrieSip, NinetyPercent>;

fn sorted_tuples_dense(relation: &HashTrieSipDense) -> Vec<Vec<usize>> {
    // `Configured` derefs to the inner `HashTrie`, so the inherent
    // `collect_tuples` is reachable unchanged.
    sorted_tuples(relation)
}

parquet_test_suite!(HashTrieSipDense, sorted_tuples_dense);
