/// Trait for reporting the number of stored tuples.
///
/// The count matches what a full iteration of the relation yields.
/// Set-semantics structures (`TreeTrie`, `ColumnTrie`) absorb duplicate
/// inserts, so their count is the number of *distinct* tuples. `HashTrie`
/// is deliberately a multiset (duplicates append to leaf chains; key
/// equality is deferred to the join algorithm), so its count includes
/// duplicates.
///
/// Query optimisers consume this via `kermit-algos`' `CatalogStats` as the
/// size signal for cardinality-aware variable ordering. Implementations
/// must be O(1) — the count is read during planning on every join.
pub trait Cardinality {
    /// Returns the number of stored tuples.
    fn tuple_count(&self) -> usize;
}
