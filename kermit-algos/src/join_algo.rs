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
    /// for this exact `query` (after any const-view rewrite), and passes
    /// each result tuple to `emit` in canonical (head-first) variable order.
    ///
    /// # Streaming
    ///
    /// This is the method every algorithm implements, and it never
    /// materialises the result. The slice handed to `emit` is borrowed for
    /// that call only, so an implementation reuses one scratch row and
    /// allocates nothing per tuple. The `iteration` and `end_to_end`
    /// benchmark metrics time exactly this path, counting rows through a
    /// `black_box` sink (issue #65). The sink is a generic parameter rather
    /// than `&mut dyn FnMut`, so the per-tuple call is monomorphised, not
    /// an indirect call inside the timed region.
    ///
    /// # Panics
    ///
    /// Implementations panic if `plan` fails
    /// [`QueryPlan::validate`] against `query` — an invalid plan is a
    /// caller programming error, never a recoverable runtime condition.
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, emit: S,
    );

    /// Joins as [`join_for_each`](JoinAlgo::join_for_each) does and returns
    /// the materialised result. Allocates one `Vec` per tuple, so it is for
    /// callers that need the rows themselves, never for a timed region.
    ///
    /// # Panics
    ///
    /// As [`join_for_each`](JoinAlgo::join_for_each).
    fn join_iter(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>,
    ) -> impl Iterator<Item = Vec<usize>> {
        let mut tuples = Vec::new();
        Self::join_for_each(plan, query, datastructures, |tuple| {
            tuples.push(tuple.to_vec())
        });
        tuples.into_iter()
    }
}
