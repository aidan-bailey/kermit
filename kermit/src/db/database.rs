//! The relation store the join entry points read.

use {
    super::{JoinFamily, RelationArities},
    kermit_algos::{distinct_per_column, RelationStats, StatisticsLevel},
    kermit_ds::{Cardinality, Relation},
    std::collections::BTreeMap,
};

/// Relations by name, plus the statistics a query optimiser reads.
///
/// A database is a catalog in the textbook sense: planners read its
/// statistics, never its relations. The statistics are gathered once, when
/// the database is built ("ANALYZE"), up to the [`StatisticsLevel`] asked
/// for, and the database has no mutation API, so they cannot drift from
/// the relations. Build it at the level the optimiser declares
/// ([`QueryOptimiser::required_statistics`]). The join entry points return
/// [`JoinError::MissingStatistics`] rather than plan from less.
///
/// [`QueryOptimiser::required_statistics`]: kermit_algos::QueryOptimiser::required_statistics
/// [`JoinError::MissingStatistics`]: super::JoinError::MissingStatistics
pub struct Database<R> {
    relations: BTreeMap<String, R>,
    statistics: BTreeMap<String, RelationStats>,
    level: StatisticsLevel,
}

impl<R: Relation + Cardinality> Database<R> {
    /// Builds a database over `relations`, keyed by the names queries use,
    /// gathering statistics up to `level`.
    ///
    /// Tuple counts come from [`Cardinality::tuple_count`], which every
    /// structure keeps, so they cost nothing.
    /// [`StatisticsLevel::ColumnDistinct`] also walks each relation once
    /// through `F` ([`JoinFamily::for_each_tuple`]) to count each column's
    /// distinct values: O(N · arity) time, O(distinct values) space.
    pub fn new<F: JoinFamily<R>>(relations: BTreeMap<String, R>, level: StatisticsLevel) -> Self {
        let mut database = Self::from(relations);
        if level >= StatisticsLevel::ColumnDistinct {
            for (name, stats) in &mut database.statistics {
                let relation = &database.relations[name];
                stats.column_distinct =
                    distinct_per_column(stats.arity, |visit| F::for_each_tuple(relation, visit));
            }
        }
        database.level = level;
        database
    }
}

impl<R> Database<R> {
    /// The relation queries call `name`, if the database holds one.
    pub fn get(&self, name: &str) -> Option<&R> { self.relations.get(name) }

    /// Every relation, in name order.
    pub fn relations(&self) -> impl Iterator<Item = &R> { self.relations.values() }

    /// The statistics gathered for `name`, if the database holds it.
    pub fn statistics(&self, name: &str) -> Option<&RelationStats> { self.statistics.get(name) }

    /// How far the statistics go.
    pub fn level(&self) -> StatisticsLevel { self.level }
}

/// A database with tuple counts only ([`StatisticsLevel::TupleCounts`]):
/// free to build, and all that an optimiser with the default
/// `required_statistics` reads.
impl<R: Relation + Cardinality> From<BTreeMap<String, R>> for Database<R> {
    fn from(relations: BTreeMap<String, R>) -> Self {
        let statistics = relations
            .iter()
            .map(|(name, relation)| {
                let stats = RelationStats::new(relation.tuple_count(), relation.header().arity());
                (name.clone(), stats)
            })
            .collect();
        Self {
            relations,
            statistics,
            level: StatisticsLevel::TupleCounts,
        }
    }
}

impl<R: Relation> RelationArities for Database<R> {
    fn arity(&self, relation: &str) -> Option<usize> { self.relations.arity(relation) }

    fn relation_names(&self) -> Vec<String> { self.relations.relation_names() }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::{HashFamily, SortedFamily},
        kermit_algos::{ColumnEquality, TrieIterKind},
        kermit_ds::{
            ColumnTrie, HashTrie, HeapSize, LazyExpansion, NoPruning, SingletonPruning, TreeTrie,
        },
        kermit_iters::{SipHashStrategy, TrieIterable},
    };

    /// A sorted family whose walk panics, so a database built at
    /// `TupleCounts` provably never walks a relation.
    struct NoWalk;

    impl<R: TrieIterable> JoinFamily<R> for NoWalk {
        type Wrapper<'a>
            = TrieIterKind<'a, R>
        where
            R: 'a;

        fn wrap_relation(_: &R) -> Self::Wrapper<'_> { unreachable!() }

        fn wrap_const<'a>(_: usize) -> Self::Wrapper<'a>
        where
            R: 'a,
        {
            unreachable!()
        }

        fn wrap_selection(_: &R, _: Vec<ColumnEquality>) -> Self::Wrapper<'_> { unreachable!() }

        fn for_each_tuple(_: &R, _: impl FnMut(&[usize])) {
            panic!("a database built at TupleCounts walked a relation")
        }
    }

    fn edges() -> Vec<Vec<usize>> { vec![vec![1, 10], vec![1, 20], vec![2, 10], vec![3, 30]] }

    fn store<R: Relation>(tuples: Vec<Vec<usize>>) -> BTreeMap<String, R> {
        BTreeMap::from([("edge".to_string(), R::from_tuples(2.into(), tuples))])
    }

    #[test]
    fn tuple_counts_read_cardinality_and_walk_nothing() {
        let database =
            Database::new::<NoWalk>(store::<TreeTrie>(edges()), StatisticsLevel::TupleCounts);
        assert_eq!(database.level(), StatisticsLevel::TupleCounts);
        assert_eq!(database.statistics("edge"), Some(&RelationStats::new(4, 2)));
    }

    #[test]
    fn a_map_converts_to_a_tuple_count_database() {
        let database = Database::from(store::<TreeTrie>(edges()));
        assert_eq!(database.level(), StatisticsLevel::TupleCounts);
        assert_eq!(database.statistics("edge"), Some(&RelationStats::new(4, 2)));
    }

    /// Every family reports the same statistics for the same tuples. The
    /// hash trie is a multiset, so a duplicate tuple adds to its tuple
    /// count but not to any column's distinct count.
    #[test]
    fn column_distinct_counts_each_column_in_every_family() {
        let level = StatisticsLevel::ColumnDistinct;
        let want = RelationStats::new(4, 2).with_column_distinct(vec![3, 3]);

        let tree = Database::new::<SortedFamily>(store::<TreeTrie>(edges()), level);
        assert_eq!(tree.level(), level);
        assert_eq!(tree.statistics("edge"), Some(&want));
        let column = Database::new::<SortedFamily>(store::<ColumnTrie>(edges()), level);
        assert_eq!(column.statistics("edge"), Some(&want));
        let hash = Database::new::<HashFamily<SipHashStrategy>>(
            store::<HashTrie<SipHashStrategy>>(edges()),
            level,
        );
        assert_eq!(hash.statistics("edge"), Some(&want));
        let pruned = Database::new::<HashFamily<SipHashStrategy>>(
            store::<HashTrie<SipHashStrategy, SingletonPruning>>(edges()),
            level,
        );
        assert_eq!(pruned.statistics("edge"), Some(&want));

        let mut duplicated = edges();
        duplicated.push(vec![1, 10]);
        let multiset = Database::new::<HashFamily<SipHashStrategy>>(
            store::<HashTrie<SipHashStrategy>>(duplicated),
            level,
        );
        assert_eq!(
            multiset.statistics("edge"),
            Some(&RelationStats::new(5, 2).with_column_distinct(vec![3, 3]))
        );
    }

    /// Statistics never probe a relation (#92). A lazy `HashTrie` builds the
    /// children a probe reaches, so a walk through `hash_trie_iter` would
    /// expand the whole trie before any join ran: laziness gone, and
    /// `space` measuring an expanded trie. The walk must lend the stored
    /// tuples instead, leaving the trie byte-for-byte as built.
    #[test]
    fn column_distinct_leaves_a_lazy_trie_as_built() {
        type Lazy = HashTrie<SipHashStrategy, NoPruning, LazyExpansion>;
        let as_built = store::<Lazy>(edges())["edge"].heap_size_bytes();
        let database = Database::new::<HashFamily<SipHashStrategy>>(
            store::<Lazy>(edges()),
            StatisticsLevel::ColumnDistinct,
        );
        assert_eq!(
            database.statistics("edge"),
            Some(&RelationStats::new(4, 2).with_column_distinct(vec![3, 3]))
        );
        assert_eq!(
            database.get("edge").unwrap().heap_size_bytes(),
            as_built,
            "gathering statistics expanded the lazy trie"
        );
    }

    #[test]
    fn relations_are_found_by_name() {
        let database = Database::from(store::<TreeTrie>(edges()));
        assert!(database.get("edge").is_some());
        assert!(database.get("nope").is_none());
        assert_eq!(database.relations().count(), 1);
        assert_eq!(database.statistics("nope"), None);
    }

    #[test]
    fn arities_agree_with_the_map_they_wrap() {
        let map = store::<TreeTrie>(edges());
        let (arity, names) = (map.arity("edge"), map.relation_names());
        let database = Database::from(map);
        assert_eq!(database.arity("edge"), arity);
        assert_eq!(database.arity("nope"), None);
        assert_eq!(database.relation_names(), names);
    }
}
