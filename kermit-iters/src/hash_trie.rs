//! Hash trie iterator family — counterpart to `TrieIterator` for hash-trie
//! join algorithms. Iterators expose hashes (not sorted keys) and navigate
//! via exact hash lookup rather than least-upper-bound seek.
//!
//! See SIGMOD 2020 "Combining Worst-Case Optimal and Traditional Binary
//! Join Processing" §3.2 for the conceptual interface (Table 1).

use crate::joinable::JoinIterable;

/// Iterator over a hash trie. Navigates nested hash tables level by level.
///
/// # Position model
///
/// Conceptually, the iterator points to a single bucket within one of the
/// nodes of a hash trie. The bucket either holds a child hash table (inner
/// levels) or a tuple chain (the leaf level).
///
/// # Method semantics
///
/// - [`key`](Self::key) — hash at the current bucket, or `None` if at end / not
///   yet opened.
/// - [`next`](Self::next) — advance to the next occupied bucket; return its
///   hash.
/// - [`lookup`](Self::lookup) — move to the bucket with exact hash `hash`;
///   return whether one exists.
/// - [`size`](Self::size) — number of occupied buckets in the current node.
/// - [`at_end`](Self::at_end) — `true` iff positioned past the last occupied
///   bucket.
/// - [`open`](Self::open) — descend into the child node at the current bucket.
/// - [`up`](Self::up) — ascend to the parent node.
/// - [`leaf_tuples`](Self::leaf_tuples) — at the leaf level, the tuple chain at
///   the current bucket; `None` at inner levels.
///
/// # Not a `LinearIterator`
///
/// `LinearIterator::seek` has least-upper-bound semantics on sorted data.
/// Hash navigation is exact-match. The two contracts are incompatible, so
/// `HashTrieIterator` is a separate trait family rather than an extension.
pub trait HashTrieIterator {
    /// Hash at the current bucket, or `None` if at end or not yet opened.
    fn key(&self) -> Option<u64>;

    /// Advance to the next occupied bucket. Returns its hash, or `None` if
    /// no further bucket exists at this level.
    fn next(&mut self) -> Option<u64>;

    /// Position at the bucket with exact hash `hash`. Returns `true` iff a
    /// matching bucket exists.
    fn lookup(&mut self, hash: u64) -> bool;

    /// Number of occupied buckets in the current node — the paper's
    /// `size(I_j)`. Used by the algorithm to pick the smallest table at
    /// each level (`argmin size`).
    fn size(&self) -> usize;

    /// `true` iff the iterator is positioned past the last occupied bucket
    /// in the current node.
    fn at_end(&self) -> bool;

    /// Descend into the child node at the current bucket. Returns `false`
    /// if already at the leaf level or if the current bucket is empty.
    fn open(&mut self) -> bool;

    /// Ascend to the parent node. Returns `false` at the root.
    fn up(&mut self) -> bool;

    /// Tuple chain at the current leaf bucket. Returns `Some(&[…])` iff the
    /// current node is a leaf and the current bucket is occupied;
    /// otherwise `None`.
    fn leaf_tuples(&self) -> Option<&[Vec<usize>]>;
}

/// Marker for types that expose a [`HashTrieIterator`].
///
/// The hash-trie counterpart of [`TrieIterable`](crate::TrieIterable). Note
/// that this trait does *not* require `IntoIterator<Item = Vec<usize>>` —
/// hash iteration over a single relation isn't naturally tuple-shaped (the
/// per-level key is a hash, not a value). Data structures implementing this
/// trait typically also provide their own depth-first tuple-materialization
/// helper for the `Projectable` impl.
pub trait HashTrieIterable: JoinIterable {
    /// Returns a `HashTrieIterator` over this structure.
    fn hash_trie_iter(&self) -> impl HashTrieIterator;
}
