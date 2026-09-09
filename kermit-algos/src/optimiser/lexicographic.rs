//! The default ordering policy: smallest canonical variable index first.

use crate::{
    analysis::analyse,
    optimiser::{ordering::topological_order, CatalogStats, QueryOptimiser, QueryPlan},
};

/// Plans the global attribute order by Kahn's topological sort with a
/// smallest-canonical-index tie-break.
///
/// This reproduces, bit for bit, the ordering kermit hardcoded before
/// query optimisers existed — it keeps head variables early when
/// unconstrained and is fully deterministic. It ignores statistics, which
/// makes it the control arm for optimiser ablation studies and the
/// default everywhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LexicographicOptimiser;

impl QueryOptimiser for LexicographicOptimiser {
    fn plan(&self, query: &kermit_parser::JoinQuery, _stats: &CatalogStats) -> QueryPlan {
        let analysis = analyse(query);
        QueryPlan {
            variable_ordering: topological_order(
                analysis.num_vars,
                &analysis.predicate_variables,
                |v| v,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_parser::JoinQuery};

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
        let stats = CatalogStats::for_query(&q, |name| match name {
            | "R" => Some(1_000_000),
            | "S" => Some(1),
            | _ => None,
        });
        let plan = LexicographicOptimiser.plan(&q, &stats);
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }
}
