//! Query optimisers: planners that choose the global attribute order.
//!
//! Kermit separates planning from execution. A `QueryOptimiser` (later in
//! this module) consumes a parsed, const-rewritten
//! [`JoinQuery`](kermit_parser::JoinQuery) plus per-relation statistics and
//! produces a `QueryPlan`; join algorithms execute the plan. This module
//! hosts the shared query analysis both sides rely on for a consistent
//! canonical variable numbering.

mod analysis;
mod ordering;
mod plan;

pub use {
    analysis::{analyse, QueryAnalysis},
    ordering::topological_order,
    plan::{PlanError, QueryPlan},
};
