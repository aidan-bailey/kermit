mod ds;
mod relation;
mod shared;

// Re-export IndexStructure for external crates (CLI) to reference directly
pub use {
    ds::{ColumnTrie, IndexStructure, TreeTrie},
    relation::{Projectable, Relation, RelationFileExt, RelationHeader},
};
