//! This module defines any kinds of nodes that could be shared amongst data
//! structures.
use kermit_iters::key_type::KeyType;

/// The `Node` trait defines a node with an associated value.
pub trait Node {
    /// The type of key stored.
    type KT: KeyType;

    /// Creates a new node with the given key.
    fn new(key: Self::KT) -> Self;

    /// Returns a reference to the key.
    fn key(&self) -> Self::KT;
}
