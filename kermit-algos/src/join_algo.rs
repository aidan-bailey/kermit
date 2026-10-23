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
    /// for this exact `query` (the executed query of `IndexPlan::query`,
    /// whose atoms name the indexes, with the plan translated to it), and
    /// passes each result tuple to `emit` in canonical (head-first) variable
    /// order.
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
    /// `plan.variable_ordering.len()`: one column per variable of `query`, in
    /// canonical order.
    ///
    /// # Panics
    ///
    /// Implementations panic if `plan` fails
    /// [`QueryPlan::validate`] against `query` — an invalid plan is a
    /// caller programming error, never a recoverable runtime condition.
    fn join_for_each<S: FnMut(&[usize])>(
        plan: &QueryPlan, query: JoinQuery, datastructures: HashMap<String, &DS>, emit: S,
    );
}
