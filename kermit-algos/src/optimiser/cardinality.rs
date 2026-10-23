//! Smallest-relation-first ordering policy.

use crate::{
    analysis::analyse,
    optimiser::{ordering::order_by_rank, CatalogStats, QueryOptimiser, QueryPlan},
};

/// Plans the global attribute order preferring variables that appear in
/// the smallest relations.
///
/// Each variable is ranked by the minimum tuple count over the body
/// predicates that mention it (a variable in a small relation can only
/// take few values, so binding it early shrinks the search space). A
/// predicate is an access, so a constant's filter has already shrunk its
/// count. Relations without statistics rank as `usize::MAX` ("assume
/// large"). Ties break on the canonical index, keeping plans
/// deterministic for reproducible benchmarks.
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
            variable_ordering: order_by_rank(analysis.num_vars, |v| min_size[v]),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::optimiser::RelationStats, kermit_parser::JoinQuery};

    fn stats_for(q: &JoinQuery, sizes: &[(&str, usize)]) -> CatalogStats {
        CatalogStats::for_query(q, |name| {
            let arity = q.body.iter().find(|p| p.name == name)?.terms.len();
            let &(_, tuples) = sizes.iter().find(|(n, _)| *n == name)?;
            Some(RelationStats::new(tuples, arity))
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
        // Const_c9 is given one tuple, so its variable K wins over X even
        // though X is canonically first and unconstrained.
        let q: JoinQuery = "Q(X, K) :- R(X), Const_c9(K).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 500), ("Const_c9", 1)]));
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

    /// The ranking alone decides: `S` is tiny, so its `Y` goes first even
    /// though `R(X, Y)` lists `X` first. An index is built with its columns
    /// in plan order, so no column order constrains the plan.
    #[test]
    fn the_smallest_relation_goes_first_regardless_of_column_order() {
        let q: JoinQuery = "Q(X, Y) :- R(X, Y), S(Y).".parse().unwrap();
        let plan = CardinalityOptimiser.plan(&q, &stats_for(&q, &[("R", 1000), ("S", 1)]));
        assert_eq!(plan.variable_ordering, vec![1, 0]);
    }
}
