//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use kermit_iters::join_iterable::JoinIterable;

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<ITB>
where
    ITB: JoinIterable,
{
    /// Joins the given iterables based on the specified join plan.
    /// Returns the resulting tuples as a vector.
    fn join(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&ITB>,
    ) -> Vec<Vec<ITB::KT>> {
        Self::join_iter(variables, rel_variables, iterables)
            .map(|tuple| tuple.into_iter().cloned().collect())
            .collect()
    }

    fn join_iter<'a>(
        variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, iterables: Vec<&'a ITB>,
    ) -> impl Iterator<Item = Vec<&'a ITB::KT>>;
}
