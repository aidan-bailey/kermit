mod common;

use {
    kermit_algos::{HashTriejoin, LeapfrogTriejoin},
    kermit_ds::{ColumnTrie, HashTrie, TreeTrie},
    kermit_iters::{FxHashStrategy, SipHashStrategy},
};

type HashTrieSip = HashTrie<SipHashStrategy>;
type HashTrieFx = HashTrie<FxHashStrategy>;

define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin);

define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin);

define_multiway_join_test_suite!(HashTrieSip, HashTriejoin);

define_multiway_join_test_suite!(HashTrieFx, HashTriejoin);
