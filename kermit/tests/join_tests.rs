mod common;

use {
    kermit_algos::leapfrog_triejoin::LeapfrogTriejoin, kermit_ds::ds::{relation_trie::RelationTrie, column_trie::ColumnTrie},
};

define_multiway_join_test_suite!(RelationTrie, LeapfrogTriejoin);

define_multiway_join_test_suite!(ColumnTrie, LeapfrogTriejoin);
