mod common;

use {
    kermit_algos::{
        CardinalityOptimiser, CostBasedOptimiser, HashTriejoin, LeapfrogTriejoin,
        LexicographicOptimiser,
    },
    kermit_ds::{
        define_build_mode_provider, define_config_provider, BinarySeek, ColumnTrie,
        ColumnTrieBuildMode, GallopingSeek, HashTrie, HashTrieConfig, LinearSeek, LoadFactor,
        SingletonPruning, Threads, TreeTrie, TreeTrieBuildMode,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};

// ── Layout aliases: hasher × pruning ────────────────────────────────────
type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;

// ── Layout aliases: seek strategy (sorted tries) ────────────────────────
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

define_multiway_join_test_suite!(TreeTrieLinear, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrieLinear, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(TreeTrieLinear, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(TreeTrieBinary, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrieBinary, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(TreeTrieBinary, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(TreeTrieGalloping, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrieGalloping, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(TreeTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser);

define_multiway_join_test_suite!(ColumnTrieLinear, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrieLinear, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(ColumnTrieLinear, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(ColumnTrieBinary, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrieBinary, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(ColumnTrieBinary, LeapfrogTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(
    ColumnTrieGalloping,
    LeapfrogTriejoin,
    LexicographicOptimiser
);
define_multiway_join_test_suite!(ColumnTrieGalloping, LeapfrogTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(ColumnTrieGalloping, LeapfrogTriejoin, CostBasedOptimiser);

define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieFxPruned, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFxPruned, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFxPruned, HashTriejoin, CostBasedOptimiser);

// ── Config axis: load factor ────────────────────────────────────────────
// The default-config invocations above are the `ds_config_load_factor: 0.7`
// baseline; these are the alternate (standard: baseline + ≥1 per flag).
define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig {
    load_factor: LoadFactor::percent(50).unwrap(),
});

define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    HalfFull
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    HalfFull
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    HalfFull
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    LexicographicOptimiser,
    HalfFull
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CardinalityOptimiser,
    HalfFull
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CostBasedOptimiser,
    HalfFull
);

// ── BuildMode axis: ColumnTrie's build ──────────────────────────────────
// The plain ColumnTrie invocations above build with the default (`bulk`);
// these run the alternate. Every mode must build the same trie (issue #84).
define_build_mode_provider!(
    Incremental,
    ColumnTrieBuildMode,
    ColumnTrieBuildMode::Incremental
);

define_multiway_join_test_suite_for_build_mode!(
    ColumnTrie,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    Incremental
);
define_multiway_join_test_suite_for_build_mode!(
    ColumnTrie,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    Incremental
);
define_multiway_join_test_suite_for_build_mode!(
    ColumnTrie,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    Incremental
);

// ── BuildMode axis: TreeTrie's parallel build ───────────────────────────
// The plain TreeTrie invocations above build serially; these build on two
// threads. Every mode must build the same trie (issue #94).
define_build_mode_provider!(
    Parallel2,
    TreeTrieBuildMode,
    TreeTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

define_multiway_join_test_suite_for_build_mode!(
    TreeTrie,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    Parallel2
);
define_multiway_join_test_suite_for_build_mode!(
    TreeTrie,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    Parallel2
);
define_multiway_join_test_suite_for_build_mode!(
    TreeTrie,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    Parallel2
);
