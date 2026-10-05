//! The default ordering policy: smallest canonical variable index first.

use crate::optimiser::{
    ordering::{topological_order, Precedence},
    CatalogStats, QueryOptimiser, QueryPlan,
};

/// Plans the global attribute order by Kahn's topological sort with a
/// smallest-canonical-index tie-break.
///
/// This reproduces, bit for bit, the ordering kermit hardcoded before
/// query optimisers existed — it keeps head variables early when
/// unconstrained and is fully deterministic. It ignores statistics, which
/// makes it the control arm for optimiser ablation studies and the
/// default everywhere. Under `--column-orders any` nothing constrains it,
/// so the plan is the canonical order itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LexicographicOptimiser;

impl QueryOptimiser for LexicographicOptimiser {
    fn plan(&self, query: &kermit_parser::JoinQuery, stats: &CatalogStats) -> QueryPlan {
        QueryPlan {
            variable_ordering: topological_order(&Precedence::for_query(query, stats), |v| v),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::optimiser::{ColumnOrderPolicy, RelationStats},
        kermit_parser::JoinQuery,
    };

    #[test]
    fn triangle_orders_head_first() {
        let q: JoinQuery = "Q(X, Y, Z) :- R(X, Y), S(Y, Z), T(X, Z).".parse().unwrap();
        let plan = LexicographicOptimiser.plan(&q, &CatalogStats::default());
        assert_eq!(plan.variable_ordering, vec![0, 1, 2]);
    }

    #[test]
    fn subject_position_constant_shape_respects_column_order() {
        // K (canonical 1) is physically first in r, so it must precede Y
        // (canonical 0) despite the head-first numbering.
        let q: JoinQuery = "Q(Y) :- r(K, Y), Const_c7(K).".parse().unwrap();
        let plan = LexicographicOptimiser.plan(&q, &CatalogStats::default());
        assert_eq!(plan.variable_ordering, vec![1, 0]);
    }

    #[test]
    fn stats_are_ignored() {
        let q: JoinQuery = "Q(X, Y) :- R(X), S(Y).".parse().unwrap();
        // Even with S tiny, lexicographic keeps canonical order.
        let stats = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |name| match name {
            | "R" => Some(RelationStats::new(1_000_000, 1)),
            | "S" => Some(RelationStats::new(1, 1)),
            | _ => None,
        });
        let plan = LexicographicOptimiser.plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    /// Under `any` the plan is the canonical order itself, even when the
    /// stored column order would have forced `Y` first.
    #[test]
    fn under_any_the_canonical_order_is_the_plan() {
        let q: JoinQuery = "Q(X, Y) :- r(Y, X).".parse().unwrap();
        let stored = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |_| None);
        assert_eq!(
            LexicographicOptimiser.plan(&q, &stored).variable_ordering,
            vec![1, 0]
        );
        let any = CatalogStats::for_query(&q, ColumnOrderPolicy::Any, |_| None);
        assert_eq!(
            LexicographicOptimiser.plan(&q, &any).variable_ordering,
            vec![0, 1]
        );
    }
}
