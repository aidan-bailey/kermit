//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use kermit_iters::join_iterable::JoinIterable;

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<ITB>
where
    ITB: JoinIterable,
{
    /// Joins the given iterables based on the specified join plan.
    /// Returns an iterator over the resulting join.
    fn join_iter(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>,
    ) -> impl Iterator<Item = Vec<ITB::KT>>;
}
