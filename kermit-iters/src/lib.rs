//! Core iterator traits for Kermit's relational algebra engine.
//!
//! This crate defines the trait hierarchy that all data structures and
//! algorithms build upon: [`LinearIterator`] for flat sorted sequences and
//! [`TrieIterator`] for hierarchical trie traversal. [`TrieIteratorWrapper`]
//! adapts any `TrieIterator` into a standard `Iterator<Item = Vec<usize>>` that
//! yields complete tuples via depth-first traversal.
//!
//! All keys are `usize` (dictionary-encoded); see [`Key`]. The marker trait
//! [`JoinIterable`] unifies data structures that may participate in joins.
//!
//! # Example
//!
//! `Vec<usize>` ships with a [`LinearIterable`] implementation, useful as a
//! minimal example of the trait:
//!
//! ```
//! use kermit_iters::{LinearIterable, LinearIterator};
//!
//! let data = vec![1_usize, 3, 5, 7];
//! let mut iter = data.linear_iter();
//!
//! // Iterators start positioned *before* the first key.
//! assert_eq!(iter.next(), Some(1));
//!
//! // `seek` advances to the least upper bound of the given key.
//! assert!(iter.seek(4));
//! assert_eq!(iter.key(), Some(5));
//!
//! assert_eq!(iter.next(), Some(7));
//! assert_eq!(iter.next(), None);
//! assert!(iter.at_end());
//! ```

#![deny(missing_docs)]

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
