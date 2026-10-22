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
//! A family directory holds one module per algorithm ([`leapfrog_join`],
//! [`leapfrog_triejoin`]), each named to match its
//! `docs/algorithms/<name>.md` page and its CLI value. An algorithm reads
//! only real indexes: a query's constants and repeated variables were
//! applied as a filter when each index was built (see
//! [`crate::extract_accesses`]).

mod leapfrog_join;
mod leapfrog_triejoin;

pub use leapfrog_triejoin::LeapfrogTriejoin;
