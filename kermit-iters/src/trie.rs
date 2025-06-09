//! This module defines the `TrieIterator` trait.

use crate::{join_iterable::JoinIterable, linear::LinearIterator};

/// The `TrieIterator` trait, designed for iterators that traverse a trie-based
/// structure.
pub trait TrieIterator<'a>: LinearIterator<'a> {
    /// If there is a child iterator at the iterator's current position,
    /// repositions at said iterator and returns `true`, otherwise returns
    /// `false`.
    ///
    /// # Note
    /// If the iterator is positioned at the end, then this proceeds as if
    /// the iterator is positioned one step backwards.
    fn open(&mut self) -> bool;

    /// If there is a parent iterator at the iterator's current position,
    /// repositions at said iterator and returns `true`, otherwise returns
    /// `false`.
    ///
    /// # Note
    ///
    /// If the iterator is positioned at the end, then this proceeds as if
    /// the iterator is positioned one step backwards.
    fn up(&mut self) -> bool;
}

/// The `TrieIterable` trait is used to specify types that can be iterated
/// through the `TrieIterable` interface, and as such used in algorithms that
/// require such an iterator.
pub trait TrieIterable: JoinIterable {
    fn trie_iter(&self) -> impl TrieIterator<'_, KT = Self::KT>;
}
