//! The relation store the join entry points read.

use {
    super::{JoinFamily, RelationArities},
    kermit_algos::{distinct_per_column, IndexSpec, RelationStats, StatisticsLevel},
    kermit_ds::{Cardinality, Relation, RelationHeader},
    std::collections::BTreeMap,
};

/// Relations by name, plus the statistics a query optimiser reads, plus
/// the reordered copies the current query needs.
///
/// A database is a catalog in the textbook sense: planners read its
/// statistics, never its relations. The statistics are gathered once, when
/// the database is built ("ANALYZE"), up to the [`StatisticsLevel`] asked
/// for, and nothing after that can change them, so they cannot drift from
/// the relations. Build it at the level the optimiser declares
/// ([`Planner::required_statistics`]). The join entry points return
/// [`JoinError::MissingStatistics`] rather than plan from less.
///
/// Under `--column-orders any` a plan may read an atom through a copy of
/// its relation with the columns permuted (`Index_<π>_<base>`, see
/// [`kermit_algos::orient`]). The catalog *stores* those copies and never
/// builds them: [`required_indexes`](Self::required_indexes) says which
/// copies a query needs, the caller builds each from the base relation's
/// file-order tuples ([`build_index`] in the library, the bench family's
/// `build_relation` in the binary) and [`add_index`](Self::add_index)es
/// it, and [`clear_indexes`](Self::clear_indexes) drops them after the
/// query, so one query's copies are held at a time. Statistics stay those
/// of the base relations: the planner sees only base names.
///
/// [`Planner::required_statistics`]: kermit_algos::Planner::required_statistics
/// [`JoinError::MissingStatistics`]: super::JoinError::MissingStatistics
pub struct Database<R> {
    relations: BTreeMap<String, R>,
    statistics: BTreeMap<String, RelationStats>,
    level: StatisticsLevel,
    /// The current query's copies, by their `Index_*` name.
    indexes: BTreeMap<String, (IndexSpec, R)>,
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

    /// The reordered copy queries call `name`, if the store holds one.
    pub fn index(&self, name: &str) -> Option<&R> { self.indexes.get(name).map(|(_, copy)| copy) }

    /// Every copy the store holds, with its spec, in name order.
    pub fn indexes(&self) -> impl Iterator<Item = (&IndexSpec, &R)> {
        self.indexes.values().map(|(spec, copy)| (spec, copy))
    }

    /// Drops every copy. The base relations and statistics stay.
    pub fn clear_indexes(&mut self) { self.indexes.clear(); }
}

impl<R: Relation> Database<R> {
    /// Stores `copy` as the relation `spec` names, replacing any copy of
    /// that name. `copy` is `spec.base` with its columns permuted by
    /// `spec.permutation`; see [`build_index`].
    ///
    /// # Panics
    ///
    /// In debug builds, if `copy`'s arity is not the permutation's length
    /// or the base relation's arity.
    pub fn add_index(&mut self, spec: IndexSpec, copy: R) {
        debug_assert_eq!(
            copy.header().arity(),
            spec.permutation.len(),
            "a copy has one column per permutation entry"
        );
        if let Some(base) = self.relations.get(&spec.base) {
            debug_assert_eq!(
                copy.header().arity(),
                base.header().arity(),
                "a copy has its base relation's arity"
            );
        }
        self.indexes.insert(spec.name.clone(), (spec, copy));
    }
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
            indexes: BTreeMap::new(),
        }
    }
}

impl<R: Relation> RelationArities for Database<R> {
    fn arity(&self, relation: &str) -> Option<usize> { self.relations.arity(relation) }

    fn relation_names(&self) -> Vec<String> { self.relations.relation_names() }
}

/// The header of `spec`'s copy: named `spec.name`, with the base's
/// attribute names permuted (or positional, if the base is).
pub fn index_header(spec: &IndexSpec, base: &RelationHeader) -> RelationHeader {
    if base.attrs().is_empty() {
        RelationHeader::new_positional(&spec.name, base.arity())
    } else {
        let attrs = spec
            .permutation
            .iter()
            .map(|&column| base.attrs()[column].clone())
            .collect();
        RelationHeader::new(&spec.name, attrs)
    }
}

/// Builds `spec`'s copy from its base relation's `tuples`, in the order
/// given (file order, for the same reason `insertion` rebuilds from it),
/// through `R::from_tuples`, so a `Configured` or `BuiltWith` relation
/// keeps its config or build mode. Never through `Projectable::project`,
/// which rebuilds in the structure's own iteration order with a nameless
/// header and drops the build mode.
pub fn build_index<R: Relation>(
    spec: &IndexSpec, base: &RelationHeader, tuples: &[Vec<usize>],
) -> R {
    R::from_tuples(index_header(spec, base), spec.permute_all(tuples))
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::db::{build_index, index_header, HashFamily, SortedFamily},
        kermit_algos::{ColumnEquality, IndexSpec, TrieIterKind},
        kermit_ds::{
            BuildModeProvider, BuiltWith, ColumnTrie, ColumnTrieBuildMode, ConfigurableRelation,
            Configured, HashTrie, HashTrieConfig, LoadFactor, RelationHeader, SingletonPruning,
            TreeTrie,
        },
        kermit_iters::{SipHashStrategy, TrieIterable},
        std::cell::Cell,
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

    #[test]
    fn an_added_copy_is_found_by_its_name_and_cleared() {
        let mut database = Database::from(store::<TreeTrie>(edges()));
        assert!(database.index("Index_1_0_edge").is_none());
        assert_eq!(database.indexes().count(), 0);

        let spec = IndexSpec::new("edge", vec![1, 0]);
        let header = RelationHeader::new_positional("edge", 2);
        let copy: TreeTrie = build_index(&spec, &header, &edges());
        database.add_index(spec.clone(), copy);
        assert!(database.index("Index_1_0_edge").is_some());
        assert!(
            database.get("Index_1_0_edge").is_none(),
            "copies are not base relations"
        );
        let held: Vec<&IndexSpec> = database.indexes().map(|(spec, _)| spec).collect();
        assert_eq!(held, vec![&spec]);
        // The statistics are read-only and know nothing of copies.
        assert_eq!(database.statistics("Index_1_0_edge"), None);

        database.clear_indexes();
        assert!(database.index("Index_1_0_edge").is_none());
        assert_eq!(database.indexes().count(), 0);
        assert!(
            database.get("edge").is_some(),
            "clearing copies keeps the base relations"
        );
    }

    #[test]
    fn index_header_permutes_named_attributes_and_keeps_positional_arity() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        let named = RelationHeader::new("edge", vec!["src".into(), "dst".into()]);
        let header = index_header(&spec, &named);
        assert_eq!(header.name(), "Index_1_0_edge");
        assert_eq!(header.attrs(), ["dst".to_string(), "src".to_string()]);
        assert_eq!(header.arity(), 2);
        let positional = index_header(&spec, &RelationHeader::new_positional("edge", 2));
        assert_eq!(positional.name(), "Index_1_0_edge");
        assert!(positional.attrs().is_empty());
        assert_eq!(positional.arity(), 2);
    }

    #[test]
    fn build_index_permutes_the_tuples_in_file_order() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        let header = RelationHeader::new_positional("edge", 2);
        let copy: ColumnTrie = build_index(&spec, &header, &edges());
        assert_eq!(copy.header().name(), "Index_1_0_edge");
        let mut stored = Vec::new();
        SortedFamily::for_each_tuple(&copy, |t| stored.push(t.to_vec()));
        assert_eq!(stored, vec![vec![10, 1], vec![10, 2], vec![20, 1], vec![
            30, 3
        ]]);
    }

    kermit_ds::define_config_provider!(HalfFull, HashTrieConfig, HashTrieConfig {
        load_factor: LoadFactor::percent(50).unwrap(),
    });

    thread_local! {
        static BUILD_CALLS: Cell<usize> = const { Cell::new(0) };
    }

    /// A build-mode provider that counts how often the build asks for it.
    struct CountedIncremental;

    impl BuildModeProvider<ColumnTrieBuildMode> for CountedIncremental {
        fn build_mode() -> ColumnTrieBuildMode {
            BUILD_CALLS.with(|calls| calls.set(calls.get() + 1));
            ColumnTrieBuildMode::Incremental
        }
    }

    /// A copy is built the way its relation type builds, so a configured
    /// hash trie keeps its load factor and a `BuiltWith` column trie asks
    /// its provider for the mode — which `Projectable::project` would not
    /// do (it rebuilds through `from_tuples` with a nameless header).
    #[test]
    fn build_index_keeps_config_and_build_mode() {
        let spec = IndexSpec::new("edge", vec![1, 0]);
        let header = RelationHeader::new_positional("edge", 2);

        let hashed: Configured<HashTrie<SipHashStrategy>, HalfFull> =
            build_index(&spec, &header, &edges());
        assert_eq!(
            hashed.config().load_factor,
            LoadFactor::percent(50).unwrap()
        );
        assert_eq!(hashed.header().name(), "Index_1_0_edge");

        BUILD_CALLS.with(|calls| calls.set(0));
        let column: BuiltWith<ColumnTrie, CountedIncremental> =
            build_index(&spec, &header, &edges());
        assert_eq!(
            BUILD_CALLS.with(Cell::get),
            1,
            "the copy asked the provider for its mode"
        );
        assert_eq!(column.header().name(), "Index_1_0_edge");
    }
}
