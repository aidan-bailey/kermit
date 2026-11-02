//! A chained hash index keyed on its leading columns: the join hash table of
//! a binary hash join, in the keyed iterator family.

mod hash_index_iter;
mod implementation;

#[cfg(test)]
mod tests;

pub use implementation::HashIndex;
