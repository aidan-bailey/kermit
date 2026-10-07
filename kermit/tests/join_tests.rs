mod common;

use {
    common::utils::AnyOrders,
    kermit_algos::{
        CardinalityOptimiser, CostBasedOptimiser, HashTriejoin, LeapfrogTriejoin,
        LexicographicOptimiser,
    },
    kermit_ds::{
        define_build_mode_provider, define_config_provider, BinarySeek, BuiltWith, ChildCapacity,
        ColumnTrie, ColumnTrieBuildMode, Configured, GallopingSeek, HashTrie, HashTrieBuildMode,
        HashTrieConfig, LazyExpansion, LinearSeek, LoadFactor, NoPruning, RadixBits, RootCapacity,
        SingletonPruning, Threads, TreeTrie, TreeTrieBuildMode,
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
    ..HashTrieConfig::default()
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

// ── Config axis: root capacity ──────────────────────────────────────────
// The default-config invocations above are the `ds_config_root_capacity:
// "grow"` baseline; these are the alternate (#88). Under `tuples` each
// fixture's root is sized once from its tuple count instead of growing from
// 4 buckets.
define_config_provider!(PresizedRoot, HashTrieConfig, HashTrieConfig {
    root_capacity: RootCapacity::Tuples,
    ..HashTrieConfig::default()
});

define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    LexicographicOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CardinalityOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CostBasedOptimiser,
    PresizedRoot
);
// The root is never lazy, so a lazy trie's presized root holds unexpanded
// children like any other.
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
    HashTriejoin,
    LexicographicOptimiser,
    PresizedRoot
);
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
    HashTriejoin,
    CostBasedOptimiser,
    PresizedRoot
);

// ── Config axis: child capacity ─────────────────────────────────────────
// The default-config invocations above are the `ds_config_child_capacity:
// "grow"` baseline; these are the alternate (#107). Under `tuples` every
// child is sized once from its list, as Algorithm 2's line 3 sizes it.
define_config_provider!(SizedChildren, HashTrieConfig, HashTrieConfig {
    child_capacity: ChildCapacity::Tuples,
    ..HashTrieConfig::default()
});

define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    SizedChildren
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    SizedChildren
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    SizedChildren
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    LexicographicOptimiser,
    SizedChildren
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CardinalityOptimiser,
    SizedChildren
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx,
    HashTriejoin,
    CostBasedOptimiser,
    SizedChildren
);
// Expansion sizes each child from its pending list, so a lazy trie's children
// are sized too.
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
    HashTriejoin,
    LexicographicOptimiser,
    SizedChildren
);
define_multiway_join_test_suite_with_config!(
    HashTrieSipLazy,
    HashTriejoin,
    CostBasedOptimiser,
    SizedChildren
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

// ── BuildMode axis: HashTrie's build ────────────────────────────────────
// The plain HashTrie invocations above build `bulk` (Algorithm 2, the
// default); these run the radix build, which must build the identical trie
// (issue #91). Two bits make four partitions, so the 3–5 tuple fixtures
// spread over several partitions with several keys in each. Sip/off/eager,
// Fx/on/eager and Sip/on/lazy cover both hashers, both pruning policies and
// both expansion policies, each under every optimiser.
define_build_mode_provider!(
    Radix2,
    HashTrieBuildMode,
    HashTrieBuildMode::Radix(RadixBits::new(2).unwrap())
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    LexicographicOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CardinalityOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CostBasedOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    LexicographicOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CardinalityOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CostBasedOptimiser,
    Radix2
);

// ── BuildMode axis: HashTrie's per-tuple build ──────────────────────────
// The plain HashTrie invocations above build `bulk` (Algorithm 2, the
// default); these run `incremental`, the build before #107, which builds
// the identical trie. The same three Layouts as the radix rows.
// ColumnTrie's provider is `Incremental`, so this one is `HashIncremental`.
// `incremental` requires `child-capacity=grow` (it cannot size a child before
// its list is known), so it runs under the default config only.
define_build_mode_provider!(
    HashIncremental,
    HashTrieBuildMode,
    HashTrieBuildMode::Incremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    HashIncremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    HashIncremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    HashIncremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    LexicographicOptimiser,
    HashIncremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CardinalityOptimiser,
    HashIncremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CostBasedOptimiser,
    HashIncremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HashIncremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CardinalityOptimiser,
    HashIncremental
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CostBasedOptimiser,
    HashIncremental
);

// ── BuildMode axis: HashTrie's parallel build ───────────────────────────
// These build on two threads, in eight partitions. Every mode must build
// the identical trie (issue #94). The same three Layouts as the radix rows
// above (Sip/off/eager, Fx/on/eager, Sip/on/lazy), under every optimiser.
// TreeTrie's provider is `Parallel2`, so this one is `HashParallel2`.
define_build_mode_provider!(
    HashParallel2,
    HashTrieBuildMode,
    HashTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    LexicographicOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CardinalityOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSip,
    HashTriejoin,
    CostBasedOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    LexicographicOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CardinalityOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPruned,
    HashTriejoin,
    CostBasedOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    LexicographicOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CardinalityOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazy,
    HashTriejoin,
    CostBasedOptimiser,
    HashParallel2
);

// ── BuildMode × Config: the presized build ──────────────────────────────
// `presized:2` requires root-capacity=tuples (#88, `PresizedRoot` above),
// so it is stacked on the presized aliases only: the paper's partitioned
// build (#94), which builds an equivalent trie (Amendment 2). The same
// three Layouts, under every optimiser. This is the stacked cell the
// standard's prerequisite rule asks for.
define_build_mode_provider!(
    HashPresized2,
    HashTrieBuildMode,
    HashTrieBuildMode::Presized(Threads::new(2).expect("2 is not zero"))
);
type HashTrieFxPrunedPresized = Configured<HashTrieFxPruned, PresizedRoot>;
type HashTrieSipPrunedLazyPresized = Configured<HashTrieSipPrunedLazy, PresizedRoot>;
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPresized,
    HashTriejoin,
    LexicographicOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPresized,
    HashTriejoin,
    CardinalityOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPresized,
    HashTriejoin,
    CostBasedOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPrunedPresized,
    HashTriejoin,
    LexicographicOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPrunedPresized,
    HashTriejoin,
    CardinalityOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieFxPrunedPresized,
    HashTriejoin,
    CostBasedOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazyPresized,
    HashTriejoin,
    LexicographicOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazyPresized,
    HashTriejoin,
    CardinalityOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazyPresized,
    HashTriejoin,
    CostBasedOptimiser,
    HashPresized2
);

// ── BuildMode × Config: sized children under every partitioned build ────
// Under `child-capacity=tuples` (#107) the partitioned builds size every
// child as `bulk` does (at build time in the radix row, at expansion in the
// two lazy rows). The last three are the closest-to-paper configuration:
// root and children sized at a load factor of 0.8, pruned and lazy, under
// `presized:2`. `presized:N` requires `root-capacity=tuples`, so only the
// paper cell (`PaperSizing`) stacks it.
define_config_provider!(PaperSizing, HashTrieConfig, HashTrieConfig {
    load_factor: LoadFactor::percent(80).unwrap(),
    root_capacity: RootCapacity::Tuples,
    child_capacity: ChildCapacity::Tuples,
});
type HashTrieSipSizedChildren = Configured<HashTrieSip, SizedChildren>;
type HashTrieSipLazySizedChildren = Configured<HashTrieSipLazy, SizedChildren>;
type HashTrieSipPrunedLazyPaper = Configured<HashTrieSipPrunedLazy, PaperSizing>;
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipSizedChildren,
    HashTriejoin,
    LexicographicOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipSizedChildren,
    HashTriejoin,
    CardinalityOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipSizedChildren,
    HashTriejoin,
    CostBasedOptimiser,
    Radix2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipLazySizedChildren,
    HashTriejoin,
    LexicographicOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipLazySizedChildren,
    HashTriejoin,
    CardinalityOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipLazySizedChildren,
    HashTriejoin,
    CostBasedOptimiser,
    HashParallel2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazyPaper,
    HashTriejoin,
    LexicographicOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazyPaper,
    HashTriejoin,
    CardinalityOptimiser,
    HashPresized2
);
define_multiway_join_test_suite_for_build_mode!(
    HashTrieSipPrunedLazyPaper,
    HashTriejoin,
    CostBasedOptimiser,
    HashPresized2
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
type HashTrieSipPresized = Configured<HashTrieSip, PresizedRoot>;
type HashTrieFxPresized = Configured<HashTrieFx, PresizedRoot>;
type HashTrieSipLazyPresized = Configured<HashTrieSipLazy, PresizedRoot>;
type TreeTrieParallel2 = BuiltWith<TreeTrie, Parallel2>;
type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;
type HashTrieSipRadix2 = BuiltWith<HashTrieSip, Radix2>;
type HashTrieSipParallel2 = BuiltWith<HashTrieSip, HashParallel2>;
type HashTrieSipPresized2 = BuiltWith<HashTrieSipPresized, HashPresized2>;
type HashTrieSipIncremental = BuiltWith<HashTrieSip, HashIncremental>;

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
    TreeTrieParallel2,
    LeapfrogTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    TreeTrieParallel2,
    LeapfrogTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    TreeTrieParallel2,
    LeapfrogTriejoin,
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
    HashTrieSipRadix2,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipRadix2,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipRadix2,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipParallel2,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipParallel2,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipParallel2,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipPresized2,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipPresized2,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipPresized2,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipPresized,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipPresized,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipPresized,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieFxPresized,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieFxPresized,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieFxPresized,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipLazyPresized,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipLazyPresized,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipSizedChildren,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipSizedChildren,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipSizedChildren,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipIncremental,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipIncremental,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipIncremental,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
    HashTrieSipPrunedLazyPaper,
    HashTriejoin,
    LexicographicOptimiser,
    AnyOrders,
    HashTrieSipPrunedLazyPaper,
    HashTriejoin,
    CardinalityOptimiser,
    AnyOrders,
    HashTrieSipPrunedLazyPaper,
    HashTriejoin,
    CostBasedOptimiser,
    AnyOrders,
);
