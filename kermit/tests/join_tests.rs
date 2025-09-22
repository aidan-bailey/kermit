mod common;

use {
    kermit_algos::leapfrog_triejoin::LeapfrogTriejoin,
    kermit_ds::{ColumnTrie, TreeTrie},
};

define_multiway_join_test_suite!(TreeTrie, LeapfrogTriejoin);

define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin);
