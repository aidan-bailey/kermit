//! Per-relation statistics: the planner's *input*, not the planner.
//!
//! Callers (the join entry points in `kermit::db`) build a
//! [`CatalogStats`] per join and hand it to
//! [`QueryOptimiser::plan`](super::QueryOptimiser::plan). Nothing here
//! plans anything, and nothing here touches a data structure — that is
//! what keeps planners data-structure-agnostic and unit-testable with
//! literal maps. [`distinct_per_column`] counts over a tuple walk the caller
//! lends it, for the same reason.

use {
    super::ColumnOrderPolicy,
    crate::const_rewrite::is_const_predicate,
    kermit_parser::JoinQuery,
    std::{
        collections::{BTreeMap, HashSet},
        fmt,
    },
};

/// How much a planner needs to know about each relation, cheapest first.
///
/// A [`QueryOptimiser`](super::QueryOptimiser) declares its level through
/// `required_statistics`, and the join engine gathers exactly that much
/// when it builds its relation store, so an optimiser that reads only tuple
/// counts never pays for a walk over the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatisticsLevel {
    /// The number of stored tuples per relation. Free: every structure
    /// keeps it (`kermit_ds::Cardinality`).
    TupleCounts,
    /// Tuple counts plus the number of distinct values in each column. One
    /// walk over every relation.
    ColumnDistinct,
}

impl fmt::Display for StatisticsLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            | StatisticsLevel::TupleCounts => "tuple counts",
            | StatisticsLevel::ColumnDistinct => "per-column distinct counts",
        })
    }
}

/// Statistics for one relation, as visible to the planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationStats {
    /// Number of stored tuples (`kermit_ds::Cardinality` semantics: what a
    /// full iteration yields).
    pub tuples: usize,
    /// Number of attributes.
    pub arity: usize,
    /// Distinct values in each column, in column order. Empty unless the
    /// statistics were gathered at [`StatisticsLevel::ColumnDistinct`].
    pub column_distinct: Vec<usize>,
}

impl RelationStats {
    /// Tuple-count statistics, with no per-column counts.
    pub fn new(tuples: usize, arity: usize) -> Self {
        Self {
            tuples,
            arity,
            column_distinct: Vec::new(),
        }
    }

    /// These statistics with one distinct-value count per column.
    ///
    /// # Panics
    ///
    /// If `column_distinct` does not hold exactly one count per column.
    pub fn with_column_distinct(self, column_distinct: Vec<usize>) -> Self {
        assert_eq!(
            column_distinct.len(),
            self.arity,
            "one distinct count per column"
        );
        Self {
            column_distinct,
            ..self
        }
    }
}

/// Counts the distinct values in each of `arity` columns over the tuples
/// `walk` lends its visitor: the statistic
/// [`StatisticsLevel::ColumnDistinct`] adds. The caller supplies the walk
/// (typically a relation's own), so this needs no data structure. One
/// `HashSet` per column: O(N · arity) time, O(distinct values) space.
pub fn distinct_per_column(
    arity: usize, walk: impl FnOnce(&mut dyn FnMut(&[usize])),
) -> Vec<usize> {
    let mut seen: Vec<HashSet<usize>> = vec![HashSet::new(); arity];
    walk(&mut |tuple| {
        for (values, &value) in seen.iter_mut().zip(tuple) {
            values.insert(value);
        }
    });
    seen.iter().map(HashSet::len).collect()
}

/// Per-relation statistics for the predicates of one query, plus the
/// column-order policy the plan is made under.
///
/// Plain data: planners never touch data structures, so they stay
/// data-structure-agnostic and unit-testable with literal maps. Callers
/// build one per join via [`CatalogStats::for_query`]. The policy rides
/// along because it decides which atoms *pin* the plan to their stored
/// column order ([`is_pinned`](Self::is_pinned)), which every optimiser
/// reads through [`Precedence::for_query`](super::Precedence::for_query).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogStats {
    relations: BTreeMap<String, RelationStats>,
    column_orders: ColumnOrderPolicy,
}

impl CatalogStats {
    /// Records stats for `name`, replacing any previous entry.
    pub fn insert(&mut self, name: impl Into<String>, stats: RelationStats) {
        self.relations.insert(name.into(), stats);
    }

    /// Tuple count for `name`, if known.
    pub fn tuples(&self, name: &str) -> Option<usize> { self.relations.get(name).map(|s| s.tuples) }

    /// Distinct values in column `column` of `name`, if they were gathered.
    pub fn distinct(&self, name: &str, column: usize) -> Option<usize> {
        self.relations
            .get(name)?
            .column_distinct
            .get(column)
            .copied()
    }

    /// Builds stats for every body predicate of `query`, under
    /// `column_orders`.
    ///
    /// `stats_of` supplies a relation's statistics (in `kermit::db`, those
    /// its `Database` gathered when it was built). Synthetic `Const_*`
    /// predicates (introduced by the const-view rewrite) are recorded as
    /// single-tuple unary relations whose one column has one value.
    /// Predicates the lookup does not know get no entry — planners treat
    /// missing stats as "assume large".
    pub fn for_query(
        query: &JoinQuery, column_orders: ColumnOrderPolicy,
        stats_of: impl Fn(&str) -> Option<RelationStats>,
    ) -> Self {
        let mut stats = CatalogStats {
            column_orders,
            ..CatalogStats::default()
        };
        for pred in &query.body {
            if stats.relations.contains_key(&pred.name) {
                continue;
            }
            if is_const_predicate(&pred.name) {
                stats.insert(
                    pred.name.clone(),
                    RelationStats::new(1, 1).with_column_distinct(vec![1]),
                );
            } else if let Some(relation) = stats_of(&pred.name) {
                stats.insert(pred.name.clone(), relation);
            }
        }
        stats
    }

    /// The column-order policy these statistics were built under.
    pub fn column_orders(&self) -> ColumnOrderPolicy { self.column_orders }

    /// Whether the atom named `name` must bind its columns left to right,
    /// i.e. adds its column-order edges to the plan's precedence graph.
    ///
    /// `stored` pins every relation and `any` pins none, so today the
    /// name is not consulted; it is the seam for a selective policy
    /// (issue #82: a relation with a stored reordered copy is free). A
    /// `Select_<n>_<base>` view follows its base, and a `Const_*`
    /// singleton is unary, so pinning it changes nothing.
    pub fn is_pinned(&self, _name: &str) -> bool {
        match self.column_orders {
            | ColumnOrderPolicy::Stored => true,
            | ColumnOrderPolicy::Any => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, kermit_parser::JoinQuery};

    #[test]
    fn statistics_levels_order_cheapest_first() {
        assert!(StatisticsLevel::TupleCounts < StatisticsLevel::ColumnDistinct);
        assert_eq!(StatisticsLevel::TupleCounts.to_string(), "tuple counts");
        assert_eq!(
            StatisticsLevel::ColumnDistinct.to_string(),
            "per-column distinct counts"
        );
    }

    #[test]
    fn for_query_records_relations_and_const_singletons() {
        let q: JoinQuery = "Q(X) :- R(X, K0), Const_c5(K0).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |name| match name {
            | "R" => Some(RelationStats::new(42, 2).with_column_distinct(vec![7, 3])),
            | _ => None,
        });
        assert_eq!(stats.tuples("R"), Some(42));
        assert_eq!(stats.distinct("R", 0), Some(7));
        assert_eq!(stats.distinct("R", 1), Some(3));
        assert_eq!(stats.tuples("Const_c5"), Some(1));
        assert_eq!(stats.distinct("Const_c5", 0), Some(1));
        assert_eq!(stats.tuples("Unknown"), None);
    }

    #[test]
    fn distinct_counts_are_absent_at_tuple_counts() {
        let q: JoinQuery = "Q(X) :- R(X).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |_| {
            Some(RelationStats::new(9, 1))
        });
        assert_eq!(stats.tuples("R"), Some(9));
        assert_eq!(stats.distinct("R", 0), None);
    }

    #[test]
    fn for_query_skips_unknown_relations() {
        let q: JoinQuery = "Q(X) :- R(X), Mystery(X).".parse().unwrap();
        let stats = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |name| {
            (name == "R").then(|| RelationStats::new(7, 1))
        });
        assert_eq!(stats.tuples("Mystery"), None);
    }

    #[test]
    fn for_query_looks_up_each_relation_once() {
        use std::cell::Cell;
        let q: JoinQuery = "Q(X, Z) :- R(X, Y), R(Y, Z).".parse().unwrap();
        let lookups = Cell::new(0);
        let stats = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |name| {
            lookups.set(lookups.get() + 1);
            (name == "R").then(|| RelationStats::new(9, 2))
        });
        assert_eq!(stats.tuples("R"), Some(9));
        assert_eq!(lookups.get(), 1);
    }

    #[test]
    fn stored_pins_every_atom_and_any_pins_none() {
        let q: JoinQuery = "Q(X) :- R(X, K0), Select_0_R(X, K1), Const_c5(K0)."
            .parse()
            .unwrap();
        let stored = CatalogStats::for_query(&q, ColumnOrderPolicy::Stored, |_| None);
        assert_eq!(stored.column_orders(), ColumnOrderPolicy::Stored);
        for atom in &q.body {
            assert!(stored.is_pinned(&atom.name), "{}", atom.name);
        }
        let any = CatalogStats::for_query(&q, ColumnOrderPolicy::Any, |_| None);
        assert_eq!(any.column_orders(), ColumnOrderPolicy::Any);
        for atom in &q.body {
            assert!(!any.is_pinned(&atom.name), "{}", atom.name);
        }
    }

    #[test]
    fn default_statistics_are_stored() {
        assert_eq!(
            CatalogStats::default().column_orders(),
            ColumnOrderPolicy::Stored
        );
    }

    #[test]
    fn distinct_per_column_counts_each_column_separately() {
        let tuples = [vec![1, 10], vec![1, 20], vec![2, 10], vec![3, 30], vec![
            3, 30,
        ]];
        let counts = distinct_per_column(2, |visit| {
            for tuple in &tuples {
                visit(tuple.as_slice());
            }
        });
        assert_eq!(counts, vec![3, 3]);
    }

    #[test]
    fn distinct_per_column_of_an_empty_walk_is_zero() {
        assert_eq!(distinct_per_column(2, |_| {}), vec![0, 0]);
    }

    #[test]
    #[should_panic(expected = "one distinct count per column")]
    fn with_column_distinct_needs_one_count_per_column() {
        let _ = RelationStats::new(4, 2).with_column_distinct(vec![1]);
    }
}
