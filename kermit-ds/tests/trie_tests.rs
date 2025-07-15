use kermit_ds::ds::{column_trie::ColumnTrie, relation_trie::RelationTrie};
mod common;

relation_trie_test_suite!(RelationTrie);

relation_trie_test_suite!(ColumnTrie);
