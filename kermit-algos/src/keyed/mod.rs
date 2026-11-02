//! The keyed join family: algorithms over
//! [`KeyedIterable`](kermit_iters::KeyedIterable) relations.
//!
//! Every module here reads a relation through an index keyed on its leading
//! columns: [`key_len`](kermit_iters::KeyedIterable::key_len) says how many,
//! [`scan`](kermit_iters::KeyedIterable::scan) lends every row, and
//! [`probe`](kermit_iters::KeyedIterable::probe) lends the rows whose key
//! equals a given one, by exact match. A probe imposes no order on the data
//! and the index has no levels to descend, so the family shares nothing with
//! an iterator that seeks or opens (rationale in `kermit-iters/src/keyed.rs`).
//!
//! An algorithm here runs over the executed query that `kermit::db::prepare`
//! builds under [`TrieLevels::ProbeKeys`](kermit_iters::TrieLevels::ProbeKeys):
//! its body atoms are in join order, each names an index over its variables
//! in column order, and each index's key is the leading columns that the
//! atoms before it bind. The algorithm reads the body left to right and
//! derives no join order of its own.
//!
//! The module shape follows the other families: one module per algorithm
//! ([`binary_hash_join`]), named to match its `docs/algorithms/<name>.md`
//! page and its CLI value.

mod binary_hash_join;

pub use binary_hash_join::BinaryHashJoin;
