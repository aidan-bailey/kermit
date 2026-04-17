//! This module defines the `Joinable` trait, which is used to specify types
//! that can be joined.

/// Marker trait for types that can participate in a join.
///
/// # Note
/// While having a deceptively simple definition, this trait plays a crucial
/// role in unifying data structures in [`kermit_iters`](crate) with algorithms
/// in `kermit_algos`. It is the common bound that iterator-producing traits
/// extend.
///
/// Each iterator trait defined in this crate extends `JoinIterable`:
/// [`LinearIterable`](crate::LinearIterable) returns a [`LinearIterator`](crate::LinearIterator)
/// via `linear_iter`, and [`TrieIterable`](crate::TrieIterable) returns a
/// [`TrieIterator`](crate::TrieIterator) via `trie_iter`. Data structures
/// implementing `Relation` (in `kermit-ds`) must also implement `JoinIterable`
/// so they can be consumed by `kermit-algos`.
pub trait JoinIterable {}
