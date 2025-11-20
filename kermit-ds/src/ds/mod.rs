mod column_trie;
mod tree_trie;

use {clap::ValueEnum, std::str::FromStr};
pub use {column_trie::ColumnTrie, tree_trie::TreeTrie};

#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum IndexStructure {
    ColumnTrie,
    TreeTrie,
}

impl FromStr for IndexStructure {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            | "column_trie" => Ok(Self::ColumnTrie),
            | "tree_trie" => Ok(Self::TreeTrie),
            | _ => Err(format!("Invalid index structure: {}", s)),
        }
    }
}
