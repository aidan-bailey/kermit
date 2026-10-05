use {
    kermit_ds::{
        define_build_mode_provider, define_config_provider, BinarySeek, BuiltWith, ColumnTrie,
        ColumnTrieBuildMode, Configured, GallopingSeek, HashTrie, HashTrieConfig, LinearSeek,
        LoadFactor, PruningPolicy, SingletonPruning, Threads, TreeTrie, TreeTrieBuildMode,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};
mod common;

// One alias per structure × seek strategy, matching `trie_tests.rs`.
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

parquet_test_suite!(TreeTrieLinear);

parquet_test_suite!(TreeTrieBinary);

parquet_test_suite!(TreeTrieGalloping);

parquet_test_suite!(ColumnTrieLinear);

parquet_test_suite!(ColumnTrieBinary);

parquet_test_suite!(ColumnTrieGalloping);

// …and under ColumnTrie's incremental BuildMode, which must load the same
// trie (issue #84).
define_build_mode_provider!(
    Incremental,
    ColumnTrieBuildMode,
    ColumnTrieBuildMode::Incremental
);

type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;

parquet_test_suite!(ColumnTrieIncremental);

// …and under TreeTrie's parallel BuildMode, which must load the same trie
// (issue #94).
define_build_mode_provider!(
    Parallel2,
    TreeTrieBuildMode,
    TreeTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

type TreeTrieParallel2 = BuiltWith<TreeTrie, Parallel2>;

parquet_test_suite!(TreeTrieParallel2);

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
