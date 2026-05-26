mod common;

use {
    kermit_algos::{HashTriejoin, LeapfrogTriejoin},
    kermit_ds::{ColumnTrie, HashTrie, TreeTrie},
};

define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin);

define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin);

define_multiway_join_test_suite!(HashTrie, HashTriejoin);
