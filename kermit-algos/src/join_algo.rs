//! This module defines the `JoinAlgo` trait, used as a base for join
//! algorithms.

use {
    crate::optimiser::QueryPlan, kermit_iters::JoinIterable, kermit_parser::JoinQuery,
    std::collections::HashMap,
};

/// The `JoinAlgo` trait is used as a base for join algorithms.
pub trait JoinAlgo<DS>
where
    DS: JoinIterable,
{
    /// Joins the given iterables according to `plan`, which must have been
    /// produced by a [`QueryOptimiser`](crate::optimiser::QueryOptimiser)
    /// for this exact `query` (after any const-view rewrite). Returns an
    /// iterator over the resulting join.
    ///
    /// # Laziness is not part of the contract
    ///
    /// The `impl Iterator` return type says nothing about *when* work
    /// happens. [`LeapfrogTriejoin`](crate::LeapfrogTriejoin) yields tuples
    /// lazily, one per `next()`, whereas
    /// [`HashTriejoin`](crate::HashTriejoin) materialises the whole result
    /// into a `Vec` before returning its iterator. Callers that measure
    /// throughput over the full result (the `lftj_join` / `hash_join` path) see
    /// no difference; a time-to-first-tuple or peak-memory metric would,
    /// and must not assume every implementation streams.
    ///
    /// # Panics
    ///
    /// Implementations panic if `plan` fails
    /// [`QueryPlan::validate`] against `query` — an invalid plan is a
    /// caller programming error, never a recoverable runtime condition.
    fn join_iter(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>>;
}
