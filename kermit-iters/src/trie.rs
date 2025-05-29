use crate::linear::LinearIterator;

/// Trie iterator trait
pub trait TrieIterator<'a, KT: PartialOrd + PartialEq + Clone>: LinearIterator<'a, KT> {
    /// If there is a non-root node at the iterator's current position which has
    /// children, positions the iterator at the first child and returns a
    /// reference to the key. Otherwise, returns None.
    ///
    /// # Note
    /// If the iterator is positioned at the end, then this functions as if
    /// the iterator is positioned at the previous node.
    fn open(&mut self) -> Option<&'a KT>;

    /// If there is a non-root node at the iterator's current position,
    /// positions the iterator at its parent and returns a reference to the key
    /// if it is a non-root Node.
    /// Otherwise, returns None.
    ///
    /// # Note
    /// If the iterator is positioned at the end, then this functions as if
    /// the iterator is positioned at the previous node.
    fn up(&mut self) -> Option<&'a KT>;
}

pub trait Iterable<KT>
where
    KT: PartialOrd + PartialEq + Clone,
{
}

/// Trie iterable trait
pub trait TrieIterable<KT: PartialOrd + PartialEq + Clone>: Iterable<KT> {
    fn trie_iter(&self) -> impl TrieIterator<'_, KT>;
}

impl<KT, T> Iterable<KT> for T
where
    T: TrieIterable<KT>,
    KT: PartialOrd + PartialEq + Clone,
{
}
