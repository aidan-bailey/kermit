use kermit_ds::ds::relation_trie::RelationTrie;
use kermit_ds::ds::column_trie::ColumnTrie;
mod common;

relation_trie_test_suite!(RelationTrie);

relation_trie_test_suite!(ColumnTrie);
