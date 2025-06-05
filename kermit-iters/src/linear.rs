use {
    crate::key_type::KeyType,
    std::{fmt::Debug, hash::Hash, str::FromStr},
};

/// Trie iterator trait
pub trait LinearIterator<'a, KT: KeyType> {
    /// Returns a reference to the key if
    /// the iterator is positioned at a
    /// non-root node, otherwise None.
    fn key(&self) -> Option<&'a KT>;

    /// Moves the iterator forward and returns
    /// a reference to the key if the iterator
    /// is positioned at a non-root node, otherwise
    /// None.
    fn next(&mut self) -> Option<&'a KT>;

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
    fn seek(&mut self, seek_key: &KT) -> Option<&'a KT>;

    /// Returns true iff the iterator is positioned
    /// at the end.
    fn at_end(&self) -> bool;
}

/// Trie iterable trait
pub trait LinearIterable<'a, KT: KeyType> {
    fn linear_iter(&'a self) -> impl LinearIterable<'a, KT>;
}
