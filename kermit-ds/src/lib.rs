mod ds;
mod relation;
mod shared;

pub use {
    ds::{ColumnTrie, TreeTrie},
    relation::{Projectable, Relation, RelationFileExt, RelationHeader},
};

// Re-export IndexStructure for external crates (CLI) to reference directly
pub use ds::IndexStructure;
