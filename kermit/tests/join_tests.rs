mod common;

use {
    kermit_algos::leapfrog_triejoin::LeapfrogTriejoin, kermit_ds::ds::relation_trie::RelationTrie,
};

define_multiway_join_test_suite!(RelationTrie, LeapfrogTriejoin);
