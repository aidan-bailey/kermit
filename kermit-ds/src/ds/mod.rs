mod column_trie;
mod hash_trie;
mod tree_trie;

// `clap::ValueEnum` is derived here, in a library crate, on purpose: it keeps
// the registry enum beside the implementations it names, so adding a structure
// touches one file for both the type and its CLI spelling. The cost is `clap`
// in this crate's dependency tree. Decided in aidan-bailey/kermit#60 (item 5).
use {clap::ValueEnum, std::str::FromStr};
pub use {
    column_trie::ColumnTrie,
    hash_trie::{
        HashTrie, HashTrieConfig, InvalidLoadFactor, LoadFactor, NoPruning, PruningPolicy,
        SingletonPruning,
    },
    tree_trie::TreeTrie,
};

/// The available trie-based index structures for storing relations.
///
/// Used as a CLI argument to select which data structure backs the join.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum IndexStructure {
    /// Column-oriented trie; see [`ColumnTrie`].
    ColumnTrie,
    /// Hash-based trie; see [`HashTrie`].
    HashTrie,
    /// Pointer-based trie; see [`TreeTrie`].
    TreeTrie,
}

impl FromStr for IndexStructure {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            | "column_trie" => Ok(Self::ColumnTrie),
            | "hash_trie" => Ok(Self::HashTrie),
            | "tree_trie" => Ok(Self::TreeTrie),
            | _ => Err(format!("Invalid index structure: {}", s)),
        }
    }
}

impl IndexStructure {
    /// The bench-report axis value for this structure (the
    /// `data_structure` key) and the label embedded in Criterion group
    /// names. This is a stable external contract: it names on-disk
    /// `target/criterion/` directories, so existing measurements are
    /// repartitioned if it changes. Equal to the `Debug` representation
    /// for historical continuity.
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::ColumnTrie => "ColumnTrie",
            | Self::HashTrie => "HashTrie",
            | Self::TreeTrie => "TreeTrie",
        }
    }
}

#[cfg(test)]
mod index_structure_tests {
    use super::*;

    /// Pins every label to a literal. These strings name
    /// `target/criterion/{group}` directories and the report's
    /// `data_structure` axis, so a change here repartitions every
    /// measurement taken so far. Change deliberately or not at all.
    #[test]
    fn axis_values_are_pinned() {
        assert_eq!(IndexStructure::ColumnTrie.axis_value(), "ColumnTrie");
        assert_eq!(IndexStructure::HashTrie.axis_value(), "HashTrie");
        assert_eq!(IndexStructure::TreeTrie.axis_value(), "TreeTrie");
    }

    /// The label used to be `format!("{:?}")`. Keeping the two equal means
    /// historical Criterion directories keep their names; if you ever
    /// diverge them, update this test *and* decide what happens to old
    /// measurements.
    #[test]
    fn axis_values_match_debug_repr() {
        for v in IndexStructure::value_variants() {
            assert_eq!(v.axis_value(), format!("{v:?}"));
        }
    }
}
