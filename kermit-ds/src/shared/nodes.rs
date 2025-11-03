//! This module defines any kinds of nodes that could be shared amongst data
//! structures.

/// The `Node` trait defines a node with an associated value.
#[allow(dead_code)]
pub trait Node {
    /// Creates a new node with the given key.
    fn new(key: usize) -> Self;

    /// Returns a reference to the key.
    fn key(&self) -> usize;
}
