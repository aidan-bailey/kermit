use {
    kermit_ds::{
        define_build_mode_provider, define_config_provider, BinarySeek, BuiltWith, ColumnTrie,
        ColumnTrieBuildMode, Configured, ExpansionPolicy, GallopingSeek, HashTrie,
        HashTrieBuildMode, HashTrieConfig, LazyExpansion, LinearSeek, LoadFactor, NoPruning,
        PruningPolicy, RadixBits, SingletonPruning, TreeTrie,
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

// `HashTrie` has no tuple-shaped iterator (it is `HashTrieIterable`, not
// `TrieIterable`), so the round-trip is checked through `collect_tuples()`,
// whose order is hash-dependent and therefore sorted before comparison. One
// invocation per Layout alias, matching `kermit/tests/join_tests.rs`.
type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;

fn sorted_tuples<H: kermit_iters::HashStrategy, P: PruningPolicy, E: ExpansionPolicy>(
    relation: &HashTrie<H, P, E>,
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

// …and under lazy child expansion: unexpanded children still yield every
// stored tuple, read from their pending lists.
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;

parquet_test_suite!(HashTrieSipLazy, sorted_tuples);

parquet_test_suite!(HashTrieFxLazy, sorted_tuples);

parquet_test_suite!(HashTrieSipPrunedLazy, sorted_tuples);

parquet_test_suite!(HashTrieFxPrunedLazy, sorted_tuples);

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

// …and under the radix BuildMode, which must load the same trie (issue #91).
define_build_mode_provider!(
    Radix2,
    HashTrieBuildMode,
    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap())
);

type HashTrieSipRadix2 = BuiltWith<HashTrieSip, Radix2>;

fn sorted_tuples_radix(relation: &HashTrieSipRadix2) -> Vec<Vec<usize>> {
    // `BuiltWith` derefs to the inner `HashTrie`, as `Configured` does.
    sorted_tuples(relation)
}

parquet_test_suite!(HashTrieSipRadix2, sorted_tuples_radix);
