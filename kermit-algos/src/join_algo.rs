//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use {crate::executed::ExecutedQuery, kermit_iters::JoinIterable};

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<DS>
where
    DS: JoinIterable,
{
    /// Joins `indexes` as `query` says, passing each result tuple to `emit`
    /// in canonical (head-first) variable order.
    ///
    /// `indexes[id]` is the index [`ExecutedQuery::atom_indexes`] names. The
    /// query is in executed form, with its plan already checked against it
    /// (see [`ExecutedQuery`]), so an implementation reads the analysis and
    /// the plan from it and checks neither. A query with no atoms has one
    /// answer, the empty row, which `emit` receives once.
    ///
    /// # Streaming
    ///
    /// This is the trait's one method, and it never materialises the result.
    /// The slice handed to `emit` is borrowed for that call only, so an
    /// implementation reuses one scratch row and never allocates a result
    /// tuple. The `iteration` and `end_to_end` benchmark metrics time
    /// exactly this path, counting rows through a `black_box` sink (issue
    /// #65). The sink is a generic parameter rather than `&mut dyn FnMut`, so
    /// the per-tuple call is monomorphised, not an indirect call inside the
    /// timed region.
    ///
    /// # Collecting
    ///
    /// To collect the result, pass a sink that pushes each row into a
    /// [`Tuples`](kermit_iters::Tuples) batch of arity
    /// `query.analysis().num_vars`: one column per variable of `query`, in
    /// canonical order.
    ///
    /// # Panics
    ///
    /// Implementations panic if `indexes.len() != query.index_count()`: the
    /// one input the type cannot check, and always a caller programming
    /// error. The joins in this crate share the check, `assert_index_count`.
    fn join_for_each<S: FnMut(&[usize])>(query: &ExecutedQuery, indexes: &[DS], emit: S);
}

/// The `# Panics` of [`JoinAlgo::join_for_each`], shared by the joins:
/// panics unless `indexes` is one index per id of `query`.
pub(crate) fn assert_index_count(algorithm: &str, query: &ExecutedQuery, indexes: usize) {
    assert!(
        indexes == query.index_count(),
        "{algorithm}::join_for_each: {indexes} indexes for a query that reads {}",
        query.index_count()
    );
}
