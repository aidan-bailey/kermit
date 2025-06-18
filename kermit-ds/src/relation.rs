//! This module defines the `Relation` and `RelationBuilder` traits.
use {
    csv::Error,
    kermit_iters::join_iterable::JoinIterable,
    std::{fs::File, path::Path},
};

/// The `Relation` trait defines a relational data structure.
pub trait Relation: JoinIterable {
    /// Creates a new relation with the specified cardinality.
    fn new(cardinality: usize) -> Self;

    /// Creates a new relation with the specified cardinality and given tuples.
    fn from_tuples(tuples: Vec<Vec<Self::KT>>) -> Self;

    /// Returns the cardinality of the relation.
    fn cardinality(&self) -> usize;

    /// Inserts a tuple into the relation, returning `true` if successful and
    /// `false` if otherwise.
    fn insert(&mut self, tuple: Vec<Self::KT>) -> bool;

    /// Inserts multiple tuples into the relation, returning `true` if
    /// successful and `false` if otherwise.
    fn insert_all(&mut self, tuples: Vec<Vec<Self::KT>>) -> bool;

    /// Creates a new relation builder with the specified cardinality.
    fn builder(cardinality: usize) -> impl RelationBuilder<Output = Self>
    where
        Self: Relation + Sized,
    {
        Builder::<Self>::new(cardinality)
    }
}

/// The `RelationBuilder` trait defines a relational data structure builder.
///
/// # Note
/// Why is this trait needed? Perhaps there is an optimised way during the
/// initialisation of a relation to build it.
pub trait RelationBuilder {
    /// The type of relation being built.
    type Output: Relation;

    /// Creates a new relation builder with the specified cardinality.
    fn new(cardinality: usize) -> Self;

    /// Consumes the builder and returns the resulting relation.
    fn build(self) -> Self::Output;

    /// Adds a tuple to the relation being built.
    fn add_tuple(self, tuple: Vec<<Self::Output as JoinIterable>::KT>) -> Self;

    /// Adds multiple tuples to the relation being built.
    fn add_tuples(self, tuples: Vec<Vec<<Self::Output as JoinIterable>::KT>>) -> Self;
}

/// A concrete, default implementation of the `RelationBuilder` trait for a
/// specific relation type `R`.
pub struct Builder<R: Relation> {
    cardinality: usize,
    tuples: Vec<Vec<R::KT>>,
}

/// Implementation of the `RelationBuilder` trait for the `Builder<R>` type.
impl<R: Relation> RelationBuilder for Builder<R> {
    type Output = R;

    fn new(cardinality: usize) -> Self {
        Builder {
            cardinality,
            tuples: vec![],
        }
    }

    fn build(self) -> Self::Output {
        let mut r = R::new(self.cardinality);
        r.insert_all(self.tuples);
        r
    }

    fn add_tuple(mut self, tuple: Vec<<Self::Output as JoinIterable>::KT>) -> Self {
        self.tuples.push(tuple);
        self
    }

    fn add_tuples(mut self, tuples: Vec<Vec<<Self::Output as JoinIterable>::KT>>) -> Self {
        self.tuples.extend(tuples);
        self
    }
}

/// Extension trait for `RelationBuilder` to add CSV file reading capabilities.
pub trait RelationBuilderFileExt: RelationBuilder {
    /// Adds tuples from a CSV file to the relation being built.
    ///
    /// # Note
    /// * The CSV file should not have headers, and the delimiter can be
    ///   specified.
    /// * Each line represents a tuple, and each value in the line should be
    ///   parsable into `Relation::KT`.
    fn add_csv<P: AsRef<Path>>(self, filepath: P, delimiter: u8) -> Result<Self, Error>
    where
        Self: Sized;
}

/// Blanket implementation of `RelationBuilderFileExt` for any type that
/// implements `RelationBuilder`.
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
            let mut tuple: Vec<<T::Output as JoinIterable>::KT> = vec![];
            for x in record.iter() {
                if let Ok(y) = x.to_string().parse::<<T::Output as JoinIterable>::KT>() {
                    tuple.push(y);
                }
            }
            self = self.add_tuple(tuple);
        }
        Ok(self)
    }
}
