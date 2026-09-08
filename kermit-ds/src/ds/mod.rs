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
    hash_trie::{HashTrie, HashTrieConfig, NoPruning, PruningPolicy, SingletonPruning},
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
