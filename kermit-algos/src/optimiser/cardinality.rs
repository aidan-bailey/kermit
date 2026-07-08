//! Smallest-relation-first ordering policy.

use crate::optimiser::{
    analysis::analyse, ordering::topological_order, CatalogStats, QueryOptimiser, QueryPlan,
};

/// Plans the global attribute order preferring variables that appear in
/// the smallest relations.
///
/// Each variable is ranked by the minimum tuple count over the body
/// predicates that mention it (a variable in a small relation can only
/// take few values, so binding it early shrinks the search space).
/// Relations without statistics rank as `usize::MAX` ("assume large");
/// synthetic `Const_*` singletons rank 1 and are therefore bound as early
/// as constraints allow — exactly the right treatment for constants. Ties
/// break on the canonical index, keeping plans deterministic for
/// reproducible benchmarks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CardinalityOptimiser;

impl QueryOptimiser for CardinalityOptimiser {
    fn plan(&self, query: &kermit_parser::JoinQuery, stats: &CatalogStats) -> QueryPlan {
        let analysis = analyse(query);
        let mut min_size = vec![usize::MAX; analysis.num_vars];
        for (pred, vars) in query.body.iter().zip(&analysis.predicate_variables) {
            let size = stats.tuples(&pred.name).unwrap_or(usize::MAX);
            for &v in vars {
                min_size[v] = min_size[v].min(size);
            }
        }
        QueryPlan {
            variable_ordering: topological_order(
                analysis.num_vars,
                &analysis.predicate_variables,
                |v| min_size[v],
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_parser::JoinQuery};

    fn stats_for(q: &JoinQuery, sizes: &[(&str, usize)]) -> CatalogStats {
        CatalogStats::for_query(q, |name| {
            sizes.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
        })
    }

    #[test]
    fn star_prefers_variable_from_smaller_relation() {
        // Q(A, B, C) :- R(A, B), S(A, C). After A, both B and C are ready;
        // S is smaller, so C (its variable) comes first.
        let q: JoinQuery = "Q(A, B, C) :- R(A, B), S(A, C).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 1000), ("S", 3)]));
        assert_eq!(plan.variable_ordering, vec![0, 2, 1]);
        // Flip the sizes: canonical order wins.
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 3), ("S", 1000)]));
        assert_eq!(plan.variable_ordering, vec![0, 1, 2]);
    }

    #[test]
    fn const_singleton_binds_first() {
        // Const_c9 has one tuple, so its variable K wins over X even
        // though X is canonically first and unconstrained.
        let q: JoinQuery = "Q(X, K) :- R(X), Const_c9(K).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 500)]));
        assert_eq!(plan.variable_ordering, vec![1, 0]);
    }

    #[test]
    fn missing_stats_rank_last() {
        // Unknown has no stats — assume large; R's variable goes first.
        let q: JoinQuery = "Q(X, Y) :- R(X), Unknown(Y).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 500)]));
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn equal_sizes_fall_back_to_canonical_order() {
        let q: JoinQuery = "Q(X, Y) :- R(X), S(Y).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 10), ("S", 10)]));
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn shared_variable_ranks_by_its_smallest_relation() {
        // X appears in both Big and Small; the min fold ranks X by
        // Small (2), beating Y's 5 — regardless of which mentioning
        // predicate comes first in the body. A max / first-wins /
        // last-wins bug would order Y first in one of the two shapes.
        let q: JoinQuery = "Q(X, Y) :- Big(X), Small(X), Other(Y).".parse().unwrap();
        let sizes: &[(&str, usize)] = &[("Big", 1000), ("Small", 2), ("Other", 5)];
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, sizes));
        assert_eq!(plan.variable_ordering, vec![0, 1]);

        let q: JoinQuery = "Q(X, Y) :- Small(X), Big(X), Other(Y).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, sizes));
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }

    #[test]
    fn column_order_constraints_still_bind() {
        // Even though Y's relation is tiny, R(X, Y) forces X first.
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 1000), ("S", 1)]));
        assert_eq!(plan.variable_ordering, vec![0, 1]);
    }
}
