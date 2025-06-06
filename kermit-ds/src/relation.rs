//! This module defines the `Relation` trait.
use kermit_iters::join_iterable::JoinIterable;

/// Trait for relations.
pub trait Relation: JoinIterable {
    /// Creates a new relation with the specified cardinality.
    fn new(cardinality: usize) -> Self;

    /// Creates a new relation with the specified cardinality and given tuples
    fn from_tuples(cardinality: usize, tuples: Vec<Vec<Self::KT>>) -> Self;

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
