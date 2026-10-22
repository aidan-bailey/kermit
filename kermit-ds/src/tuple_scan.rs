/// Lends every stored tuple to a visitor **without probing the structure**.
///
/// Probing is not always read-only: a `HashTrie` under `LazyExpansion`
/// builds each child the first probe reaches (issue #92). A tuple walk
/// driven through the structure's iterator therefore expands every lazy
/// child on the way, so a caller that only needs the stored tuples would
/// build the whole trie before any join ran. A scan builds nothing:
/// implementations visit the tuples as stored, change nothing, and allocate
/// nothing per tuple.
///
/// Implemented by the hash family, the one family whose probes can mutate.
/// A sorted trie's iterator is read-only, so a caller walks those through
/// it instead.
pub trait TupleScan {
    /// Visits every stored tuple once (duplicates included, for a
    /// multiset), in the structure's native order.
    fn scan_tuples(&self, visit: impl FnMut(&[usize]));
}
