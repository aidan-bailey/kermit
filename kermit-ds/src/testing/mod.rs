mod macros;
mod utils;

#[cfg(test)]
mod tests {
    use crate::{define_trie_relation_test_suite, ds::relation_trie::RelationTrie};

    define_trie_relation_test_suite!(RelationTrie);
}
