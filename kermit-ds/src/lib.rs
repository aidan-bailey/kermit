mod ds;
mod relation;
mod shared;

// Re-export IndexStructure for external crates (CLI) to reference directly
pub use ds::IndexStructure;
pub use {
    ds::{ColumnTrie, TreeTrie},
    relation::{Projectable, Relation, RelationFileExt, RelationHeader},
};
