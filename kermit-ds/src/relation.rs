/// ! This module defines the `Relation` trait.
use crate::key_type::KeyType;

/// Trait for relations.
pub trait Relation {
    /// The type of value stored in a relation's tuples.
    type KT: KeyType;

    /// Returns the cardinality of the relation, which is the number of tuples
    /// it contains.
    fn cardinality(&self) -> usize;

    /// Inserts a tuple into the relation, returning `true` if successful and
    /// `false` if otherwise.
    fn insert(&mut self, tuple: Vec<Self::KT>) -> bool;

    /// Inserts multiple tuples into the relation, returning `true` if
    /// successful and `false` if otherwise.
    fn insert_all(&mut self, tuples: Vec<Vec<Self::KT>>) -> bool;
}
