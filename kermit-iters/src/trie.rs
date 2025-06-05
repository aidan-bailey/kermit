use crate::{join_iterable::JoinIterable, key_type::KeyType};

/// Trie iterator trait
pub trait TrieIterator<'a> {
    type KT: KeyType;

    /// Returns a reference to the key if
    /// the iterator is positioned at a
    /// non-root node, otherwise None.
    fn key(&self) -> Option<&'a Self::KT>;

    /// If there is a non-root node at the iterator's current position which has
    /// children, positions the iterator at the first child and returns a
    /// reference to the key. Otherwise, returns None.
    ///
    /// # Note
    /// If the iterator is positioned at the end, then this functions as if
    /// the iterator is positioned at the previous node.
    fn open(&mut self) -> Option<&'a Self::KT>;

    /// If there is a non-root node at the iterator's current position,
    /// positions the iterator at its parent and returns a reference to the key
    /// if it is a non-root Node.
    /// Otherwise, returns None.
    ///
    /// # Note
    /// If the iterator is positioned at the end, then this functions as if
    /// the iterator is positioned at the previous node.
    fn up(&mut self) -> Option<&'a Self::KT>;

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

/// Trie iterable trait
pub trait TrieIterable: JoinIterable {
    fn trie_iter(&self) -> impl TrieIterator<'_, KT = Self::KT>;
}

// impl<T> Iterable for T
// where
// T: TrieIterable,
// {
// type KT = T::KT;
// }
