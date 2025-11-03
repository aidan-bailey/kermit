//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use kermit_iters::JoinIterable;

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<DS>
where
    DS: JoinIterable,
{
    /// Joins the given iterables based on the specified join plan.
    /// Returns an iterator over the resulting join.
    fn join_iter(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, datastructures: Vec<&DS>,
    ) -> impl Iterator<Item = Vec<usize>>;
}
