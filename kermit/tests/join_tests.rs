mod common;

use {
    kermit_algos::{CardinalityOptimiser, HashTriejoin, LeapfrogTriejoin, LexicographicOptimiser},
    kermit_ds::{
        define_config_provider, ColumnTrie, HashTrie, HashTrieConfig, LoadFactor, SingletonPruning,
        TreeTrie,
    },
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};

// ── Layout aliases: hasher × pruning ────────────────────────────────────
type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;
type HashTrieSipPruned = HashTrie<SipHashStrategy, SingletonPruning>;
type HashTrieFxPruned = HashTrie<FxHashStrategy, SingletonPruning>;

define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSipPruned, HashTriejoin, CardinalityOptimiser);
define_multiway_join_test_suite!(HashTrieFxPruned, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFxPruned, HashTriejoin, CardinalityOptimiser);

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
