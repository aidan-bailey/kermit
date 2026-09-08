mod common;

use {
    kermit_algos::{CardinalityOptimiser, HashTriejoin, LeapfrogTriejoin, LexicographicOptimiser},
    kermit_ds::{define_config_provider, ColumnTrie, HashTrie, HashTrieConfig, TreeTrie},
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};

type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;

define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieSip, HashTriejoin, CardinalityOptimiser);

define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, LexicographicOptimiser);
define_multiway_join_test_suite!(HashTrieFx, HashTriejoin, CardinalityOptimiser);

// ── Config axis: singleton pruning ──────────────────────────────────────
// The four invocations above are the `ds_config_singleton_pruning: false`
// baseline; these four are the alternate, per the standard's Config test
// obligation (baseline + ≥1 alternate per flag).
define_config_provider!(PruningOn, HashTrieConfig, HashTrieConfig {
    singleton_pruning: true,
});

define_multiway_join_test_suite_with_config!(
    HashTrieSip, HashTriejoin, LexicographicOptimiser, PruningOn
);
define_multiway_join_test_suite_with_config!(
    HashTrieSip, HashTriejoin, CardinalityOptimiser, PruningOn
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx, HashTriejoin, LexicographicOptimiser, PruningOn
);
define_multiway_join_test_suite_with_config!(
    HashTrieFx, HashTriejoin, CardinalityOptimiser, PruningOn
);
