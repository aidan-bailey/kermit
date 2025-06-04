//! This module provides a trait for building relations, including methods for
//! adding tuples and reading from CSV files.

use {
    crate::relation::Relation,
    csv::Error,
    std::{fs::File, path::Path},
};

/// Trait for building relations.
pub trait RelationBuilder {
    /// The type of relation being built.
    type Output: Relation;

    /// Creates a new relation builder with the specified cardinality.
    fn new(cardinality: usize) -> Self;

    /// Consumes the builder and returns the relation.
    fn build(self) -> Self::Output;

    /// Adds a tuple to the relation being built.
    fn add_tuple(self, tuple: Vec<<Self::Output as Relation>::KT>) -> Self;

    /// Adds multiple tuples to the relation being built.
    fn add_tuples(self, tuple: Vec<Vec<<Self::Output as Relation>::KT>>) -> Self;
}

/// Extension trait for `RelationBuilder` to add CSV file reading capabilities.
pub trait RelationBuilderFileExt: RelationBuilder {

    /// Adds tuples from a CSV file to the relation being built.
    /// 
    /// # Note
    /// * The CSV file should not have headers, and the delimiter can be specified.
    /// * Each line represents a tuple, and each value in the line should be parsable into `Relation::KT`. 
    fn add_csv<P: AsRef<Path>>(self, filepath: P, delimiter: u8) -> Result<Self, Error>
    where
        Self: Sized;
}

/// Blanket implementation of `RelationBuilderFileExt` for any type that implements `RelationBuilder`.
impl<T> RelationBuilderFileExt for T
where
    T: RelationBuilder,
{
    fn add_csv<P: AsRef<Path>>(mut self, filepath: P, delimiter: u8) -> Result<Self, Error> {
        let file = File::open(filepath)?;
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            //.delimiter(b',')
            .delimiter(delimiter)
            .double_quote(false)
            .escape(Some(b'\\'))
            .flexible(false)
            .comment(Some(b'#'))
            .from_reader(file);
        for result in rdr.records() {
            let record = result?;
            let mut tuple: Vec<<T::Output as Relation>::KT> = vec![];
            for x in record.iter() {
                if let Ok(y) = x.to_string().parse::<<T::Output as Relation>::KT>() {
                    tuple.push(y);
                }
            }
            self = self.add_tuple(tuple);
        }
        Ok(self)
    }
}
