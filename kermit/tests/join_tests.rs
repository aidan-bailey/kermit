mod common;

use {
    common::utils::AnyOrders,
    kermit_algos::{
        CardinalityOptimiser, CostBasedOptimiser, HashTriejoin, LeapfrogTriejoin,
        LexicographicOptimiser,
    },
    kermit_ds::{
        define_build_mode_provider, define_config_provider, BinarySeek, BuiltWith, ColumnTrie,
        ColumnTrieBuildMode, Configured, GallopingSeek, HashTrie, HashTrieConfig, LazyExpansion,
        LinearSeek, LoadFactor, NoPruning, SingletonPruning, TreeTrie,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};

// ── Layout aliases: hasher × pruning × expansion ───────────────────────
type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;
type HashTrieSipLazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
type HashTrieFxLazy = HashTrie<FxHashStrategy, NoPruning, LazyExpansion>;
type HashTrieSipPrunedLazy = HashTrie<SipHashStrategy, SingletonPruning, LazyExpansion>;
type HashTrieFxPrunedLazy = HashTrie<FxHashStrategy, SingletonPruning, LazyExpansion>;

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
define_multiway_join_test_suite!(HashTrieSipLazy, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipLazy, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSipLazy, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieFxLazy, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFxLazy, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFxLazy, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieSipPrunedLazy, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipPrunedLazy, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSipPrunedLazy, HashTriejoin, CostBasedOptimiser);
define_multiway_join_test_suite!(HashTrieFxPrunedLazy, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFxPrunedLazy, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFxPrunedLazy, HashTriejoin, CostBasedOptimiser);

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
// Expansion builds each child under the configured cap too.
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HalfFull
);
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
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

// ── Column orders: every alias × optimiser under `any` (issue #93) ─────
// The invocations above are the `stored` baseline; these run the same 16
// patterns with the planner free and the join reading reordered copies,
// which must return the same rows.
define_multiway_join_test_suite_with_column_orders!(
    TreeTrieLinear,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    TreeTrieLinear,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    TreeTrieLinear,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    TreeTrieBinary,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    TreeTrieBinary,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    TreeTrieBinary,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    TreeTrieGalloping,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    TreeTrieGalloping,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    TreeTrieGalloping,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    ColumnTrieLinear,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    ColumnTrieLinear,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    ColumnTrieLinear,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    ColumnTrieBinary,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    ColumnTrieBinary,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    ColumnTrieBinary,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    ColumnTrieGalloping,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    ColumnTrieGalloping,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    ColumnTrieGalloping,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieFx,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieFx,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieFx,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipPruned,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipPruned,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipPruned,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieFxPruned,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieFxPruned,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieFxPruned,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipLazy,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipLazy,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipLazy,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieFxLazy,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieFxLazy,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieFxLazy,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipPrunedLazy,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieFxPrunedLazy,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieFxPrunedLazy,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieFxPrunedLazy,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
);

// A copy is built the way its relation type builds, so it keeps the
// alias's config and build mode: the Config and BuildMode aliases above,
// under `any`.
type HashTrieSipHalfFull = Configured<HashTrieSip, HalfFull>;
type HashTrieFxHalfFull = Configured<HashTrieFx, HalfFull>;
type HashTrieSipLazyHalfFull = Configured<HashTrieSipLazy, HalfFull>;
type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;

define_multiway_join_test_suite_with_column_orders!(
    HashTrieSipHalfFull,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipHalfFull,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipHalfFull,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieFxHalfFull,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieFxHalfFull,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieFxHalfFull,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipLazyHalfFull,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipLazyHalfFull,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    ColumnTrieIncremental,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    ColumnTrieIncremental,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    ColumnTrieIncremental,
    LeapfrogTriejoin,
    CostBasedOptimiser,
    AnyOrders,
);
