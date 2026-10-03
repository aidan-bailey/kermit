use kermit_ds::{
    define_build_mode_provider, BinarySeek, BuiltWith, ColumnTrie, ColumnTrieBuildMode,
    GallopingSeek, LinearSeek, TreeTrie,
};
mod common;

// One alias per structure × seek strategy: the Layout test obligation of
// `docs/specs/optimization-standard.md`. `TreeTrieBinary` is plain
// `TreeTrie`.
type TreeTrieLinear = TreeTrie<LinearSeek>;
type TreeTrieBinary = TreeTrie<BinarySeek>;
type TreeTrieGalloping = TreeTrie<GallopingSeek>;
type ColumnTrieLinear = ColumnTrie<LinearSeek>;
type ColumnTrieBinary = ColumnTrie<BinarySeek>;
type ColumnTrieGalloping = ColumnTrie<GallopingSeek>;

relation_trie_test_suite!(TreeTrieLinear, TreeTrieBinary, TreeTrieGalloping);

relation_trie_test_suite!(ColumnTrieLinear, ColumnTrieBinary, ColumnTrieGalloping);

// The incremental BuildMode must satisfy the same contract: it builds the
// same trie as the default bulk build (issue #84).
define_build_mode_provider!(
    Incremental,
    ColumnTrieBuildMode,
    ColumnTrieBuildMode::Incremental
);

type ColumnTrieIncremental = BuiltWith<ColumnTrie, Incremental>;

relation_trie_test_suite!(ColumnTrieIncremental);
