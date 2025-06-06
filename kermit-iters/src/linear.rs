use crate::{join_iterable::JoinIterable, key_type::KeyType};

/// Trie iterator trait
pub trait LinearIterator<'a> {
    type KT: KeyType;

    /// Returns a reference to the key if
    /// the iterator is positioned at a
    /// non-root node, otherwise None.
    fn key(&self) -> Option<&'a Self::KT>;

    /// Moves the iterator forward and returns
    /// a reference to the key if the iterator
    /// is positioned at a non-root node, otherwise
    /// None.
    fn next(&mut self) -> Option<&'a Self::KT>;

    /// Positions the iterator at a least
    /// upper bound for seek_key,
    /// i.e. the least key ≥ seek_key,
    /// and returns a reference to the key, or
    /// move to end if no such key exists and
    /// returns None.
    ///
    /// # Panics
    ///
    /// If the seek_key is not ≥ the key at the
    /// current position.
    fn seek(&mut self, seek_key: &Self::KT) -> Option<&'a Self::KT>;

    /// Returns true iff the iterator is positioned
    /// at the end.
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

    fn seek(&mut self, seek_key: &Self::KT) -> Option<&'a Self::KT> {
        while let Some(key) = self.key() {
           if key >= seek_key {
              return Some(key);
           }
           self.index += 1;
        }
        None
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
        assert_eq!(iter.seek(&3), Some(&3));
        assert_eq!(iter.next(), Some(&4));
        assert!(!iter.at_end());
        assert_eq!(iter.next(), Some(&5));
        assert_eq!(iter.next(), None);
        assert!(iter.at_end());
    }
}
