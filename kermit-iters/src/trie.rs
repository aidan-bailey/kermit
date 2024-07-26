/// Trie iterator interface.
pub trait TrieIterator<KT: PartialOrd + PartialEq  + Clone> {
    /// If the cursor does not point to the root, returns the key of the node,
    /// otherwise returns Err.
    fn key(&self) -> Result<&KT, &'static str>;
    /// Proceeds to the next key
    fn next(&mut self) -> Result<(), &'static str>;
    /// Position the iterator at a least
    /// upper bound for seekKey,
    /// i.e. the least key ≥ seekKey, or
    /// move to end if no such key exists.
    /// The sought key must be ≥ the
    /// key at the current position.
    fn seek(&mut self, seek_key: &KT) -> Result<(), &'static str>;
    /// True when iterator is at the end.
    fn at_end(&self) -> bool;
    /// Proceed to the first key at the
    /// next depth
    fn open(&mut self) -> Result<(), &'static str>;
    /// Return to the parent key at the
    /// previous depth
    fn up(&mut self) -> Result<(), &'static str>;
}
