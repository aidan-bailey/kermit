//! This module defines the Linear iterator trait along with its implementation
//! for vectors.

use crate::{join_iterable::JoinIterable, key_type::KeyType};

/// The Linear iterator trait, designed for iterators that traverse a linear
/// structure
///
/// # Note
/// The linear iterator should be initialised one item before the first item,
/// i.e., `next` returns the first item on the first call.
pub trait LinearIterator<'a> {
    /// The key type for the iterator.
    type KT: KeyType;

    /// Returns a reference to the key at the iterator's current position,
    /// otherwise `None` if `next` has not yet been called, or the iterator
    /// is positioned at the end.
    fn key(&self) -> Option<&'a Self::KT>;

    /// Moves the iterator forward and returns
    /// a reference to the key at the new position.
    fn next(&mut self) -> Option<&'a Self::KT>;

    /// Positions the iterator at a least
    /// upper bound for the `seek_key`,
    /// i.e., the smallest key ≥ `seek_key`, or
    /// moves it to the end if no such key exists.
    fn seek(&mut self, seek_key: &Self::KT) -> bool;

    /// Returns `true` iff the iterator is positioned
    /// at the end, i.e., one after the last key.
    fn at_end(&self) -> bool;
}

/// Linear iterable trait
pub trait LinearIterable: JoinIterable {
    fn linear_iter(&self) -> impl LinearIterator<'_, KT = Self::KT>;
}

pub struct VecLinearIterator<'a, KT: KeyType> {
    data: &'a [KT],
    index: usize,
}

impl<'a, KT: KeyType> LinearIterator<'a> for VecLinearIterator<'a, KT> {
    type KT = KT;

    fn key(&self) -> Option<&'a Self::KT> {
        if self.index == 0 {
            None // No key at the start
        } else if !self.at_end() {
            Some(&self.data[self.index - 1]) // Return the last key returned by
                                             // next
        } else {
            None // At the end, no key available
        }
    }

    fn next(&mut self) -> Option<&'a Self::KT> {
        if self.at_end() {
            return None;
        }
        self.index += 1;
        self.key()
    }

    fn seek(&mut self, seek_key: &Self::KT) -> bool {
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

impl<KT: KeyType> JoinIterable for Vec<KT> {
    type KT = KT;
}

impl<KT> LinearIterable for Vec<KT>
where
    KT: KeyType,
{
    fn linear_iter(&self) -> impl LinearIterator<'_, KT = KT> {
        VecLinearIterator {
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

        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert!(iter.seek(&3));
        assert_eq!(iter.next(), Some(&4));
        assert!(!iter.at_end());
        assert_eq!(iter.next(), Some(&5));
        assert_eq!(iter.next(), None);
        assert!(iter.at_end());
    }
}
