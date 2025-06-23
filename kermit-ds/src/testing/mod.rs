mod utils;
mod macros;

#[cfg(test)]
mod tests {
    use crate::{define_trie_relation_test_suite, ds::relation_trie::RelationTrie, testing::utils};

    define_trie_relation_test_suite!(
       RelationTrie
    );


}
