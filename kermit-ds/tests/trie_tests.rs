use kermit_ds::{ColumnTrie, TreeTrie};
mod common;

relation_trie_test_suite!(TreeTrie);

relation_trie_test_suite!(ColumnTrie);
