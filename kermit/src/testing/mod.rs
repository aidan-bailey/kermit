pub mod utils;
pub mod macros;

#[cfg(test)]
mod tests {

    use kermit_algos::leapfrog_triejoin::LeapfrogTriejoin;
    use kermit_ds::ds::relation_trie::RelationTrie;

    use crate::define_multiway_join_test_suite;

    define_multiway_join_test_suite!(
        RelationTrie,
        LeapfrogTriejoin,
    );

}
