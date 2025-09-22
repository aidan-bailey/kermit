use kermit_ds::ds::{column_trie::ColumnTrie, tree_trie::TreeTrie};
mod common;

relation_trie_test_suite!(TreeTrie);

relation_trie_test_suite!(ColumnTrie);
