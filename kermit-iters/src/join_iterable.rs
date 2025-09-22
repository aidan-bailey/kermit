//! This module defines the `JoinIterable` trait, which is used to specify types
//! that can be joined.

use crate::key_type::KeyType;

/// The `JoinIterable` trait is used to specify types that can be joined.
///
/// # Note
/// While having a deceptively simple definition, this trait plays a crucial
/// role in unifying data structures found in the `kermit_iters` crate with the
/// algorithms of `kermit_algos`. It can be viewed as a sort of bridging type.
///
/// Each iterator defined in `kermit_iters` has an associated trait that extends
/// `JoinIterable`. For the `LinearIterator`, it is the trait `LinearIterable`,
/// which defines the `linear_iter` method returning something that implements
/// the `LinearIterator`. For the `TrieIterator`, it is the trait
/// `TrieIterable`, which defines the `trie_iter` method returning something
/// that implements the `TrieIterator`.
///
/// On the data structure side, all things that conform to the `Relation` trait
/// must implement `JoinIterable`. In the case of `TreeTrie`, `TrieIterable`
/// is implemented. TODO: Complete this
pub trait JoinIterable {
    /// The key type for the iterable.
    type KT: KeyType;
}
