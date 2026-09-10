//! The sorted-trie join family: algorithms over
//! [`TrieIterable`](kermit_iters::TrieIterable) relations.
//!
//! Every module here traverses relations through
//! [`TrieIterator`](kermit_iters::TrieIterator), whose
//! [`seek`](kermit_iters::LinearIterator::seek) is least-upper-bound and
//! therefore requires sorted data. Nothing in this directory refers to
//! anything in [`crate::hash`], and nothing there refers back — the two
//! families are disjoint by construction (see "Two Iterator Families" in
//! `ARCHITECTURE.md`).
//!
//! A family directory holds four kinds of module:
//!
//! - [`singleton`] — the unary one-value relation standing in for a constant
//!   atom, per the [const-view rewrite](crate::const_rewrite).
//! - [`selection`] — the equality-selection view standing in for an atom
//!   with a repeated variable, per the
//!   [selection rewrite](crate::selection_rewrite).
//! - [`iter_kind`] — the wrapper letting an algorithm hold real relations,
//!   synthetic singletons and selection views under one type.
//! - one module per algorithm ([`leapfrog_join`], [`leapfrog_triejoin`]), each
//!   named to match its `docs/algorithms/<name>.md` page and its CLI value.

mod iter_kind;
mod leapfrog_join;
mod leapfrog_triejoin;
mod selection;
mod singleton;

pub use {
    iter_kind::TrieIterKind, leapfrog_triejoin::LeapfrogTriejoin,
    selection::EqualitySelectionTrieIter, singleton::SingletonTrieIter,
};
