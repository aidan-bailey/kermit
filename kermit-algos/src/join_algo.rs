//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use {crate::JoinQuery, kermit_iters::JoinIterable};
use std::collections::HashMap;

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<DS>
where
    DS: JoinIterable,
{
    /// Joins the given iterables based on the specified join plan.
    /// Returns an iterator over the resulting join.
    fn join_iter(
        query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>>;
}
