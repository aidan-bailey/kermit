//! This module defines the `TrieIterator` trait.

use crate::{joinable::JoinIterable, linear::LinearIterator};

/// The `TrieIterator` trait, designed for iterators that traverse a trie-based
/// structure.
pub trait TrieIterator: LinearIterator {
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
    fn trie_iter(&self) -> impl TrieIterator + IntoIterator<Item = Vec<usize>>;
}

pub struct TrieIteratorWrapper<IT>
where
    IT: TrieIterator,
{
    iter: IT,
    stack: Vec<usize>,
}

impl<IT> TrieIteratorWrapper<IT>
where
    IT: TrieIterator,
{
    pub fn new(iter: IT) -> Self {
        TrieIteratorWrapper {
            iter,
            stack: vec![],
        }
    }

    fn up(&mut self) -> bool {
        if self.iter.up() {
            self.stack.pop();
            true
        } else {
            false
        }
    }

    fn down(&mut self) -> bool {
        if !self.iter.open() {
            return false;
        }
        self.stack.push(self.iter.key().unwrap());
        true
    }

    fn next_wrapper(&mut self) -> bool {
        if self.iter.at_end() {
            false
        } else if let Some(key) = self.iter.next() {
            self.stack.pop();
            self.stack.push(key);
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<Vec<usize>> {
        if !self.stack.is_empty() {
            while !self.next_wrapper() {
                if !self.up() {
                    return None;
                }
            }
        }

        while self.down() {}

        if self.stack.is_empty() {
            None
        } else {
            Some(self.stack.clone())
        }
    }
}

impl<IT> Iterator for TrieIteratorWrapper<IT>
where
    IT: TrieIterator,
{
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> { self.next() }
}
