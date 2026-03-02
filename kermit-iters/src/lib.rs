//! Core iterator traits for Kermit's relational algebra engine.
//!
//! This crate defines the trait hierarchy that all data structures and
//! algorithms build upon: [`LinearIterator`] for flat sorted sequences and
//! [`TrieIterator`] for hierarchical trie traversal. [`TrieIteratorWrapper`]
//! adapts any `TrieIterator` into a standard `Iterator<Item = Vec<usize>>` that
//! yields complete tuples via depth-first traversal.

mod joinable;
mod key_type;
mod linear;
mod trie;

pub use {
    joinable::JoinIterable,
    key_type::Key,
    linear::{LinearIterable, LinearIterator},
    trie::{TrieIterable, TrieIterator, TrieIteratorWrapper},
};
