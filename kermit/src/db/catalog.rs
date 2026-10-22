//! The flat relation store a query is prepared and built from.

use {
    super::RelationArities,
    kermit_algos::{distinct_per_column, RelationStats, StatisticsLevel},
    kermit_ds::{RelationHeader, Tuples},
    std::collections::BTreeMap,
};

/// One relation: its header, its rows, and the statistics gathered from
/// them when the catalog was created.
struct Entry {
    header: RelationHeader,
    tuples: Tuples,
    statistics: RelationStats,
}

/// Relations by name, each as a header and one flat [`Tuples`] batch, plus
/// the statistics a query optimiser reads.
///
/// A catalog is the untimed side of a query's life: relations are loaded
/// into it once, and every query then builds the indexes it needs from
/// these rows ([`TupleView`](kermit_iters::TupleView)s over them) and
/// times only that build. It holds no data structure, so the cost of
/// loading does not depend on which structure a query later builds.
///
/// The statistics are gathered once, in [`new`](Self::new), up to the
/// [`StatisticsLevel`] asked for, and cannot change afterwards, so they
/// cannot drift from the rows. Create it at the level the optimiser
/// declares ([`Planner::required_statistics`]); a planner that asks for
/// more than the catalog holds is rejected by `prepare` with
/// [`JoinError::MissingStatistics`](super::JoinError::MissingStatistics).
///
/// [`Planner::required_statistics`]: kermit_algos::Planner::required_statistics
pub struct Catalog {
    relations: BTreeMap<String, Entry>,
    level: StatisticsLevel,
}

impl Catalog {
    /// Creates a catalog over `relations`, keyed by each header's
    /// [`name`](RelationHeader::name). A later relation with the same name
    /// replaces an earlier one.
    ///
    /// Tuple counts are the row counts of the batches, so they cost
    /// nothing. [`StatisticsLevel::ColumnDistinct`] also walks each
    /// relation's rows once to count each column's distinct values:
    /// O(N · arity) time, O(distinct values) space.
    ///
    /// An empty batch is accepted under any header, as in
    /// `Relation::from_tuples`: it is replaced by an empty batch of the
    /// header's arity, so `tuples` and the statistics agree with the header.
    ///
    /// # Panics
    ///
    /// If a batch with rows has an arity other than its header's.
    pub fn new(relations: Vec<(RelationHeader, Tuples)>, level: StatisticsLevel) -> Catalog {
        let mut held: BTreeMap<String, (RelationHeader, Tuples)> = BTreeMap::new();
        for (header, tuples) in relations {
            let tuples = if tuples.is_empty() && tuples.arity() != header.arity() {
                Tuples::new(header.arity())
            } else {
                tuples
            };
            assert_eq!(
                tuples.arity(),
                header.arity(),
                "Catalog::new: tuple arity {} does not match header arity {} of relation '{}'",
                tuples.arity(),
                header.arity(),
                header.name(),
            );
            held.insert(header.name().to_string(), (header, tuples));
        }
        // Statistics are gathered after the names are settled, so a relation
        // that a later duplicate replaced is never walked.
        let relations = held
            .into_iter()
            .map(|(name, (header, tuples))| {
                let arity = header.arity();
                let mut statistics = RelationStats::new(tuples.len(), arity);
                if level >= StatisticsLevel::ColumnDistinct {
                    let column_distinct = distinct_per_column(arity, |visit| {
                        for row in tuples.rows() {
                            visit(row);
                        }
                    });
                    statistics = statistics.with_column_distinct(column_distinct);
                }
                let entry = Entry {
                    header,
                    tuples,
                    statistics,
                };
                (name, entry)
            })
            .collect();
        Catalog {
            relations,
            level,
        }
    }

    /// The header of the relation called `name`, if the catalog holds one.
    pub fn header(&self, name: &str) -> Option<&RelationHeader> {
        self.relations.get(name).map(|entry| &entry.header)
    }

    /// The rows of the relation called `name`, if the catalog holds one.
    pub fn tuples(&self, name: &str) -> Option<&Tuples> {
        self.relations.get(name).map(|entry| &entry.tuples)
    }

    /// The statistics gathered for `name`, if the catalog holds it.
    pub fn statistics(&self, name: &str) -> Option<&RelationStats> {
        self.relations.get(name).map(|entry| &entry.statistics)
    }

    /// How far the statistics go.
    pub fn level(&self) -> StatisticsLevel { self.level }

    /// Every relation name, in name order.
    pub fn names(&self) -> impl Iterator<Item = &str> { self.relations.keys().map(String::as_str) }
}

impl RelationArities for Catalog {
    fn arity(&self, relation: &str) -> Option<usize> {
        self.header(relation).map(RelationHeader::arity)
    }

    fn relation_names(&self) -> Vec<String> { self.names().map(str::to_string).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `edge/2` with named attributes: four rows, three distinct values in
    /// each column.
    fn edge() -> (RelationHeader, Tuples) {
        (
            RelationHeader::new("edge", vec!["src".into(), "dst".into()]),
            Tuples::from(vec![vec![1, 10], vec![1, 20], vec![2, 10], vec![3, 30]]),
        )
    }

    /// `r/3` with positional attributes: two rows.
    fn r() -> (RelationHeader, Tuples) {
        (
            RelationHeader::new_positional("r", 3),
            Tuples::from(vec![vec![1, 2, 3], vec![1, 2, 4]]),
        )
    }

    #[test]
    fn relations_are_found_by_header_name() {
        let catalog = Catalog::new(vec![r(), edge()], StatisticsLevel::TupleCounts);
        assert_eq!(catalog.header("edge").unwrap().attrs(), ["src", "dst"]);
        assert_eq!(catalog.tuples("edge").unwrap().len(), 4);
        assert_eq!(catalog.tuples("r").unwrap().arity(), 3);
        assert!(catalog.header("nope").is_none());
        assert!(catalog.tuples("nope").is_none());
        assert!(catalog.statistics("nope").is_none());
    }

    #[test]
    fn names_come_back_sorted_whatever_the_input_order() {
        let catalog = Catalog::new(vec![r(), edge()], StatisticsLevel::TupleCounts);
        assert_eq!(catalog.names().collect::<Vec<_>>(), vec!["edge", "r"]);
    }

    #[test]
    fn tuple_counts_hold_row_counts_and_no_column_counts() {
        let catalog = Catalog::new(vec![edge(), r()], StatisticsLevel::TupleCounts);
        assert_eq!(catalog.level(), StatisticsLevel::TupleCounts);
        assert_eq!(catalog.statistics("edge"), Some(&RelationStats::new(4, 2)));
        assert_eq!(catalog.statistics("r"), Some(&RelationStats::new(2, 3)));
    }

    #[test]
    fn column_distinct_counts_each_column() {
        let catalog = Catalog::new(vec![edge(), r()], StatisticsLevel::ColumnDistinct);
        assert_eq!(catalog.level(), StatisticsLevel::ColumnDistinct);
        assert_eq!(
            catalog.statistics("edge"),
            Some(&RelationStats::new(4, 2).with_column_distinct(vec![3, 3]))
        );
        assert_eq!(
            catalog.statistics("r"),
            Some(&RelationStats::new(2, 3).with_column_distinct(vec![1, 1, 2]))
        );
    }

    /// The catalog is flat: a repeated row is a row, so it adds to the row
    /// count and not to any column's distinct count.
    #[test]
    fn a_repeated_row_counts_as_a_row_but_not_as_a_distinct_value() {
        let (header, mut tuples) = edge();
        tuples.push(&[1, 10]);
        let catalog = Catalog::new(vec![(header, tuples)], StatisticsLevel::ColumnDistinct);
        assert_eq!(
            catalog.statistics("edge"),
            Some(&RelationStats::new(5, 2).with_column_distinct(vec![3, 3]))
        );
    }

    #[test]
    fn an_empty_relation_has_zero_counts() {
        let empty = (RelationHeader::new_positional("e", 2), Tuples::new(2));
        let catalog = Catalog::new(vec![empty], StatisticsLevel::ColumnDistinct);
        assert_eq!(
            catalog.statistics("e"),
            Some(&RelationStats::new(0, 2).with_column_distinct(vec![0, 0]))
        );
    }

    /// An empty batch has no arity of its own (`Tuples::from(vec![])` is
    /// nullary), so it is accepted under any header and takes its arity.
    #[test]
    fn an_empty_batch_is_accepted_under_any_header() {
        let header = RelationHeader::new_positional("e", 2);
        let empty = Tuples::from(Vec::<Vec<usize>>::new());
        assert_eq!(empty.arity(), 0);
        let catalog = Catalog::new(vec![(header, empty)], StatisticsLevel::ColumnDistinct);
        assert_eq!(catalog.tuples("e").unwrap().arity(), 2);
        assert_eq!(catalog.tuples("e").unwrap().len(), 0);
        assert_eq!(
            catalog.statistics("e"),
            Some(&RelationStats::new(0, 2).with_column_distinct(vec![0, 0]))
        );
    }

    #[test]
    #[should_panic(expected = "Catalog::new: tuple arity 3 does not match header arity 2")]
    fn a_batch_with_rows_of_another_arity_than_its_header_panics() {
        let header = RelationHeader::new_positional("bad", 2);
        let rows = Tuples::from(vec![vec![1, 2, 3]]);
        let _ = Catalog::new(vec![(header, rows)], StatisticsLevel::TupleCounts);
    }

    /// As `Database`'s map does, the last relation with a name wins, with
    /// its own header, tuples and statistics.
    #[test]
    fn a_later_duplicate_name_replaces_the_earlier_relation() {
        let later = (
            RelationHeader::new_positional("edge", 3),
            Tuples::from(vec![vec![7, 8, 9]]),
        );
        let catalog = Catalog::new(vec![edge(), later], StatisticsLevel::ColumnDistinct);
        assert_eq!(catalog.names().count(), 1);
        assert_eq!(catalog.header("edge").unwrap().arity(), 3);
        assert_eq!(catalog.tuples("edge").unwrap().to_vecs(), vec![vec![
            7, 8, 9
        ]]);
        assert_eq!(
            catalog.statistics("edge"),
            Some(&RelationStats::new(1, 3).with_column_distinct(vec![1, 1, 1]))
        );
    }

    #[test]
    fn arities_and_names_are_those_of_the_headers() {
        let catalog = Catalog::new(vec![r(), edge()], StatisticsLevel::TupleCounts);
        assert_eq!(catalog.arity("edge"), Some(2));
        assert_eq!(catalog.arity("r"), Some(3));
        assert_eq!(catalog.arity("nope"), None);
        assert_eq!(catalog.relation_names(), vec![
            "edge".to_string(),
            "r".to_string()
        ]);
    }
}
