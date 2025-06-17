pub mod utils;
pub mod macros;

#[cfg(test)]
mod tests {

    use kermit_algos::leapfrog_triejoin::LeapfrogTriejoin;
    use kermit_ds::ds::relation_trie::RelationTrie;

    use crate::define_join_test_suite;

    define_join_test_suite!(
        test_join_leapfrog_triejoin,
        RelationTrie<u64>,
        LeapfrogTriejoin,
    );

}
