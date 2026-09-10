//! The hash-trie join family: algorithms over
//! [`HashTrieIterable`](kermit_iters::HashTrieIterable) relations.
//!
//! Every module here traverses relations through
//! [`HashTrieIterator`](kermit_iters::HashTrieIterator), whose
//! [`lookup`](kermit_iters::HashTrieIterator::lookup) is exact-match on a
//! `u64` hash and imposes no order on the data. That contract cannot merge
//! with the sorted family's least-upper-bound `seek`, which is why this is
//! a parallel family rather than a subtrait (rationale in
//! `kermit-iters/src/hash_trie.rs`, citing SIGMOD 2020). Nothing in this
//! directory refers to anything in [`crate::sorted`], and nothing there
//! refers back.
//!
//! The module shape mirrors [`crate::sorted`] exactly — a [`singleton`],
//! a [`selection`], an [`iter_kind`], and one module per algorithm
//! ([`hash_triejoin`]) — so the two families stay comparable at a glance.

mod hash_triejoin;
mod iter_kind;
mod selection;
mod singleton;

pub use {
    hash_triejoin::HashTriejoin, iter_kind::HashTrieIterKind,
    selection::EqualitySelectionHashTrieIter, singleton::SingletonHashTrieIter,
};
