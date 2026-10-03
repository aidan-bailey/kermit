use kermit_ds::{define_build_mode_provider, BuiltWith, ColumnTrie, ColumnTrieBuildMode, TreeTrie};
mod common;

relation_trie_test_suite!(TreeTrie);

relation_trie_test_suite!(ColumnTrie);

// The incremental BuildMode must satisfy the same contract: it builds the
// same trie as the default bulk build (issue #84).
define_build_mode_provider!(
    Incremental,
    ColumnTrieBuildMode,
    ColumnTrieBuildMode::Incremental
);

type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;

relation_trie_test_suite!(ColumnTrieIncremental);
