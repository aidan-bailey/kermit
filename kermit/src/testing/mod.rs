pub mod macros;
pub mod utils;

#[cfg(test)]
mod tests {

    use {
        crate::define_multiway_join_test_suite, kermit_algos::leapfrog_triejoin::LeapfrogTriejoin,
        kermit_ds::ds::relation_trie::RelationTrie,
    };

    define_multiway_join_test_suite!(RelationTrie, LeapfrogTriejoin);
}
