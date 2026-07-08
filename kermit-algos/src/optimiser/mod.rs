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

mod analysis;
mod cardinality;
mod lexicographic;
mod ordering;
mod plan;

pub use {
    analysis::{analyse, QueryAnalysis},
    cardinality::CardinalityOptimiser,
    lexicographic::LexicographicOptimiser,
    ordering::topological_order,
    plan::{PlanError, QueryPlan},
};
use {kermit_parser::JoinQuery, std::collections::BTreeMap};

/// Statistics for one relation, as visible to the planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationStats {
    /// Number of stored tuples (`kermit_ds::Cardinality` semantics: what a
    /// full iteration yields).
    pub tuples: usize,
    /// Number of attributes.
    pub arity: usize,
}

/// Per-relation statistics for the predicates of one query.
///
/// Plain data: planners never touch data structures, so they stay
/// data-structure-agnostic and unit-testable with literal maps. Callers
/// build one per join via [`CatalogStats::for_query`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogStats {
    relations: BTreeMap<String, RelationStats>,
}

impl CatalogStats {
    /// Records stats for `name`, replacing any previous entry.
    pub fn insert(&mut self, name: impl Into<String>, stats: RelationStats) {
        self.relations.insert(name.into(), stats);
    }

    /// Tuple count for `name`, if known.
    pub fn tuples(&self, name: &str) -> Option<usize> { self.relations.get(name).map(|s| s.tuples) }

    /// Builds stats for every body predicate of `query`.
    ///
    /// `tuple_count_of` supplies per-relation counts (typically from
    /// `kermit_ds::Cardinality::tuple_count`). Synthetic `Const_*`
    /// predicates (introduced by the const-view rewrite) are recorded as
    /// single-tuple unary relations. Predicates the lookup does not know
    /// get no entry — planners treat missing stats as "assume large".
    pub fn for_query(query: &JoinQuery, tuple_count_of: impl Fn(&str) -> Option<usize>) -> Self {
        let mut stats = CatalogStats::default();
        for pred in &query.body {
            if stats.relations.contains_key(&pred.name) {
                continue;
            }
            if crate::const_rewrite::is_const_predicate(&pred.name) {
                stats.insert(pred.name.clone(), RelationStats {
                    tuples: 1,
                    arity: 1,
                });
            } else if let Some(tuples) = tuple_count_of(&pred.name) {
                stats.insert(pred.name.clone(), RelationStats {
                    tuples,
                    arity: pred.terms.len(),
                });
            }
        }
        stats
    }
}

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

#[cfg(test)]
mod tests {
    use {super::*, kermit_parser::JoinQuery};

    #[test]
    fn for_query_records_relations_and_const_singletons() {
        let q: JoinQuery = "Q(X) :- R(X, K0), Const_c5(K0).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, |name| match name {
            | "R" => Some(42),
            | _ => None,
        });
        assert_eq!(stats.tuples("R"), Some(42));
        assert_eq!(stats.tuples("Const_c5"), Some(1));
        assert_eq!(stats.tuples("Unknown"), None);
    }

    #[test]
    fn for_query_skips_unknown_relations() {
        let q: JoinQuery = "Q(X) :- R(X), Mystery(X).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, |name| (name == "R").then_some(7));
        assert_eq!(stats.tuples("Mystery"), None);
    }

    #[test]
    fn for_query_looks_up_each_relation_once() {
        use std::cell::Cell;
        let q: JoinQuery = "Q(X, Z) :- R(X, Y), R(Y, Z).".parse().unwrap();
        let lookups = Cell::new(0);
        let stats = CatalogStats::for_query(&q, |name| {
            lookups.set(lookups.get() + 1);
            (name == "R").then_some(9)
        });
        assert_eq!(stats.tuples("R"), Some(9));
        assert_eq!(lookups.get(), 1);
    }
}
