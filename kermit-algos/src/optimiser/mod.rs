//! Query optimisers: planners that choose the global attribute order.
//!
//! Kermit separates planning from execution. A [`QueryOptimiser`] consumes
//! a parsed, const-rewritten [`JoinQuery`] plus per-relation statistics
//! ([`CatalogStats`]) and produces a [`QueryPlan`]; join algorithms
//! (`JoinAlgo::join_iter`) execute the plan. The space of valid plans is
//! exactly the set of topological orders of the column-order constraint
//! DAG (see [`topological_order`]); the provided optimisers are valid by
//! construction, and executors defensively assert
//! [`QueryPlan::validate`].
//!
//! [`QueryPlan`] is the *only* type crossing from planner to executor.
//! The canonical variable numbering both sides agree on is not a planning
//! decision and lives one level up, in [`crate::analysis`].

mod cardinality;
mod lexicographic;
mod ordering;
mod plan;
mod stats;

pub use {
    cardinality::CardinalityOptimiser,
    lexicographic::LexicographicOptimiser,
    ordering::topological_order,
    plan::{PlanError, QueryPlan},
    stats::{CatalogStats, RelationStats},
};
use {clap::ValueEnum, kermit_parser::JoinQuery};

/// A query optimiser plans how a join executes.
///
/// Implementations receive the query *after* the const-view rewrite (the
/// exact query the executor will run, including synthetic `Const_*`
/// predicates) and produce a [`QueryPlan`] the executor consumes. Object
/// safe: the engine holds `Box<dyn QueryOptimiser>` so the choice is a
/// runtime decision. The returned plan must order variables consistently
/// with every relation's physical column order (any [`topological_order`]
/// output qualifies); executors assert [`QueryPlan::validate`] and panic
/// on violation.
pub trait QueryOptimiser {
    /// Produces a plan for `query` given per-relation `stats`.
    fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan;
}

/// The available query optimisers.
///
/// Used as a CLI argument to select which [`QueryOptimiser`] plans the
/// join's variable ordering. Distinct from the optimization *axes*
/// standard (`ds_layout_*` etc.) — the optimiser is a first-class
/// benchmark dimension with its own `optimiser` report axis.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
pub enum Optimiser {
    /// Smallest canonical variable index first — reproduces the
    /// pre-optimiser hardcoded ordering. The default.
    Lexicographic,
    /// Smallest-relation-first; see [`CardinalityOptimiser`].
    Cardinality,
}

impl Optimiser {
    /// Boxes the corresponding [`QueryOptimiser`] implementation.
    pub fn instantiate(self) -> Box<dyn QueryOptimiser> {
        match self {
            | Self::Lexicographic => Box::new(LexicographicOptimiser),
            | Self::Cardinality => Box::new(CardinalityOptimiser),
        }
    }

    /// The bench-report axis value for this optimiser (the `optimiser`
    /// key).
    pub fn axis_value(self) -> &'static str {
        match self {
            | Self::Lexicographic => "lexicographic",
            | Self::Cardinality => "cardinality",
        }
    }
}

#[cfg(test)]
mod optimiser_enum_tests {
    use super::*;

    /// Pins `axis_value` to clap's derived kebab-case value name, so a
    /// variant rename cannot silently desync the CLI value from the
    /// bench-report `optimiser` axis.
    #[test]
    fn axis_values_match_clap_value_names() {
        for v in Optimiser::value_variants() {
            assert_eq!(v.axis_value(), v.to_possible_value().unwrap().get_name());
        }
    }
}
