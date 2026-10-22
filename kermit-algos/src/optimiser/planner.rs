//! The planner: the optimiser a query is planned with.

use {
    super::{CatalogStats, QueryOptimiser, QueryPlan, StatisticsLevel},
    kermit_parser::JoinQuery,
    std::fmt,
};

/// What plans a join: a [`QueryOptimiser`].
///
/// The join entry points (`kermit::db`), the CLI's families and the test
/// harness all pass one of these where they used to pass a bare
/// `&dyn QueryOptimiser`. `kermit::db::prepare` plans a query once, before
/// anything is timed. No column order constrains the plan, since an index is
/// built with its columns in plan order, so a planner is its optimiser and
/// nothing else.
pub struct Planner {
    optimiser: Box<dyn QueryOptimiser>,
}

impl Planner {
    /// A planner running `optimiser`.
    pub fn new(optimiser: Box<dyn QueryOptimiser>) -> Self {
        Self {
            optimiser,
        }
    }

    /// The statistics the optimiser reads
    /// ([`QueryOptimiser::required_statistics`]).
    pub fn required_statistics(&self) -> StatisticsLevel { self.optimiser.required_statistics() }

    /// Plans `query` from `stats` ([`QueryOptimiser::plan`]).
    pub fn plan(&self, query: &JoinQuery, stats: &CatalogStats) -> QueryPlan {
        self.optimiser.plan(query, stats)
    }
}

impl fmt::Debug for Planner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Planner")
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
    fn the_planner_forwards_its_optimisers_required_statistics() {
        let planner = Planner::new(Box::new(NeedsColumns));
        assert_eq!(
            planner.required_statistics(),
            StatisticsLevel::ColumnDistinct
        );
    }

    #[test]
    fn the_planner_plans_with_its_optimiser() {
        let q: JoinQuery = "Q(A, B, C) :- R(A, B), S(A, C).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, |name| {
            let tuples = if name == "R" {
                1000
            } else {
                3
            };
            Some(RelationStats::new(tuples, 2))
        });
        let planner = Planner::new(Box::new(CardinalityOptimiser));
        assert_eq!(
            planner.plan(&q, &stats),
            CardinalityOptimiser.plan(&q, &stats)
        );
        assert_eq!(planner.plan(&q, &stats).variable_ordering, vec![0, 2, 1]);
    }

    #[test]
    fn debug_names_the_statistics() {
        let shown = format!("{:?}", Planner::new(Box::new(NeedsColumns)));
        assert!(shown.contains("ColumnDistinct"), "{shown}");
        assert!(!shown.contains("column_orders"), "{shown}");
    }
}
