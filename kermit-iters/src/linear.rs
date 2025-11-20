//! This module defines the `LinearIterator` trait along with its implementation
//! for vectors.

use crate::joinable::JoinIterable;

/// The `LinearIterator` trait, designed for iterators that traverse a linear
/// structure.
///
/// # Note
/// The linear iterator should be initialised one item before the first item,
/// i.e., `next` returns the first item on the first call.
pub trait LinearIterator {
    /// Returns the key at the iterator's current position,
    /// otherwise `None` if `next` has not yet been called, or the iterator
    /// is positioned at the end.
    fn key(&self) -> Option<usize>;

    /// Moves the iterator forward and returns
    /// the key at the new position.
    fn next(&mut self) -> Option<usize>;

    /// Positions the iterator at a least
    /// upper bound for the `seek_key`,
    /// i.e., the smallest key ≥ `seek_key`, or
    /// moves it to the end if no such key exists.
    /// Returns `true` iff the key exists.
    fn seek(&mut self, seek_key: usize) -> bool;

    /// Returns `true` iff the iterator is positioned
    /// at the end, i.e., one after the last key.
    fn at_end(&self) -> bool;
}

/// The `LinearIterable` trait is used to specify types that can be iterated
/// through the `LinearIterator` interface, and as such used in algorithms that
/// require such an iterator.
pub trait LinearIterable: JoinIterable {
    /// Returns a linear iterator for the type.
    fn linear_iter(&self) -> impl LinearIterator;
}

/// `Joinable` implementation for `Vec<usize>` informing the type system that
/// it may be used in some kind of join operation.
impl JoinIterable for Vec<usize> {}

/// A linear iterator for vectors.
struct VecLinearIter<'a> {
    data: &'a [usize],
    index: usize,
}

/// Implementation of the `LinearIterator` trait for `VecLinearIter`.
impl LinearIterator for VecLinearIter<'_> {
    fn key(&self) -> Option<usize> {
        if self.index != 0 && !self.at_end() {
            Some(self.data[self.index - 1])
        } else {
            None
        }
    }

    fn next(&mut self) -> Option<usize> {
        self.index += 1;
        if self.at_end() {
            return None;
        }
        self.key()
    }

    fn seek(&mut self, seek_key: usize) -> bool {
        while let Some(key) = self.key() {
            if key >= seek_key {
                return true;
            }
            self.index += 1;
        }
        false
    }

    fn at_end(&self) -> bool { self.index > self.data.len() }
}

/// Implementation of the `LinearIterable` trait for `Vec<KT>` informing the
/// type system that `Vec<KT>` can be used for joins requiring `LinearIterator`
/// implementations.
impl LinearIterable for Vec<usize> {
    fn linear_iter(&self) -> impl LinearIterator {
        VecLinearIter {
            data: self,
            index: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_linear_iterator() {
        let data = vec![1, 2, 3, 4, 5];
        let mut iter = data.linear_iter();

        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert!(iter.seek(3));
        assert_eq!(iter.next(), Some(4));
        assert!(!iter.at_end());
        assert_eq!(iter.next(), Some(5));
        assert_eq!(iter.next(), None);
        assert!(iter.at_end());
    }
}
