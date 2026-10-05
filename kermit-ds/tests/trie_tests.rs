use kermit_ds::{
    define_build_mode_provider, BinarySeek, BuiltWith, ColumnTrie, ColumnTrieBuildMode,
    GallopingSeek, LinearSeek, Threads, TreeTrie, TreeTrieBuildMode,
};
mod common;

// One alias per structure × seek strategy: the Layout test obligation of
// `docs/specs/optimization-standard.md`. `TreeTrieGalloping` is plain
// `TreeTrie`, the default.
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

// The parallel BuildMode must satisfy the same contract: it builds the same
// trie as the serial build (issue #94).
define_build_mode_provider!(
    Parallel2,
    TreeTrieBuildMode,
    TreeTrieBuildMode::Parallel(Threads::new(2).expect("2 is not zero"))
);

type TreeTrieParallel2 = BuiltWith<TreeTrie, Parallel2>;

relation_trie_test_suite!(TreeTrieParallel2);
