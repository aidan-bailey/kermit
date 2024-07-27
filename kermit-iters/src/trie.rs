/// Trie iterator interface.
pub trait TrieIterator<KT: PartialOrd + PartialEq + Clone> {
    /// Returns key of a Node iff not at the end or at Root.
    fn key(&self) -> Option<&KT>;
    /// Proceeds to the next key
    fn next(&mut self) -> Option<&KT>;
    /// Position the iterator at a least
    /// upper bound for seekKey,
    /// i.e. the least key ≥ seekKey, or
    /// move to end if no such key exists.
    /// The sought key must be ≥ the
    /// key at the current position.
    fn seek(&mut self, seek_key: &KT) -> Option<&KT>;
    /// True when iterator is at the end.
    fn at_end(&self) -> bool;
    /// Proceed to the first key at the
    /// next depth
    fn open(&mut self) -> Option<&KT>;
    /// Return to the parent key at the
    /// previous depth
    fn up(&mut self) -> Option<&KT>;
}

pub trait TrieIterable<KT: PartialOrd + PartialEq + Clone> {
    fn trie_iter(&self) -> impl TrieIterator<KT>;
}
