//! The planner: an optimiser plus the column-order policy it plans under.

use {
    super::{CatalogStats, ColumnOrderPolicy, QueryOptimiser, QueryPlan, StatisticsLevel},
    kermit_parser::JoinQuery,
    std::fmt,
};

/// What plans a join: a [`QueryOptimiser`] and the [`ColumnOrderPolicy`]
/// it may plan under.
///
/// The join entry points (`kermit::db`), the CLI's families and the test
/// harness all pass one of these where they used to pass a bare
/// `&dyn QueryOptimiser`, so the two planner settings travel together and
/// cannot disagree between the call that computes a query's copies and
/// the join itself. The catalog stays policy-free: it only stores the
/// copies the planner asks for.
pub struct Planner {
    optimiser: Box<dyn QueryOptimiser>,
    column_orders: ColumnOrderPolicy,
}

impl Planner {
    /// A planner running `optimiser` under `column_orders`.
    pub fn new(optimiser: Box<dyn QueryOptimiser>, column_orders: ColumnOrderPolicy) -> Self {
        Self {
            optimiser,
            column_orders,
        }
    }

    /// `optimiser` under [`ColumnOrderPolicy::Stored`]: today's planning,
    /// and the fixtures' one-liner.
    pub fn stored(optimiser: impl QueryOptimiser + 'static) -> Self {
        Self::new(Box::new(optimiser), ColumnOrderPolicy::Stored)
    }

    /// The column orders this planner may bind an atom's columns in.
    pub fn column_orders(&self) -> ColumnOrderPolicy { self.column_orders }

    /// The statistics the optimiser reads
    /// ([`QueryOptimiser::required_statistics`]).
    pub fn required_statistics(&self) -> StatisticsLevel { self.optimiser.required_statistics() }

    /// Plans `query` from `stats` ([`QueryOptimiser::plan`]). `stats` must
    /// have been built under this planner's policy
    /// ([`CatalogStats::for_query`]), which is how the policy reaches the
    /// optimiser's precedence graph.
    pub fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
        debug_assert_eq!(
            stats.column_orders(),
            self.column_orders,
            "the statistics were built under another column-order policy"
        );
        self.optimiser.plan(query, stats)
    }
}

impl fmt::Debug for Planner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Planner")
            .field("column_orders", &self.column_orders)
            .field("required_statistics", &self.required_statistics())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::optimiser::{CardinalityOptimiser, LexicographicOptimiser, RelationStats},
    };

    /// Plans like `lexicographic` but declares that it reads per-column
    /// distinct counts.
    struct NeedsColumns;

    impl QueryOptimiser for NeedsColumns {
        fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
            LexicographicOptimiser.plan(query, stats)
        }

        fn required_statistics(&self) -> StatisticsLevel { StatisticsLevel::ColumnDistinct }
    }

    #[test]
    fn stored_wraps_the_optimiser_under_the_stored_policy() {
        let planner = Planner::stored(NeedsColumns);
        assert_eq!(planner.column_orders(), ColumnOrderPolicy::Stored);
        assert_eq!(
            planner.required_statistics(),
            StatisticsLevel::ColumnDistinct
        );
    }

    #[test]
    fn the_planner_plans_with_its_optimiser() {
        let q: JoinQuery = "Q(A, B, C) :- R(A, B), S(A, C).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |name| {
            let tuples = if name == "R" {
                1000
            } else {
                3
            };
            Some(RelationStats::new(tuples, 2))
        });
        let planner = Planner::stored(CardinalityOptimiser);
        assert_eq!(
            planner.plan(&q, &stats),
            CardinalityOptimiser.plan(&q, &stats)
        );
        assert_eq!(planner.plan(&q, &stats).variable_ordering, vec![0, 2, 1]);
    }

    #[test]
    fn debug_names_the_policy_and_statistics() {
        let shown = format!(
            "{:?}",
            Planner::new(Box::new(NeedsColumns), ColumnOrderPolicy::Any)
        );
        assert!(shown.contains("Any"), "{shown}");
        assert!(shown.contains("ColumnDistinct"), "{shown}");
    }
}
