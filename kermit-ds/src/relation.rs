//! This module defines the `Relation` and `RelationBuilder` traits.
use {
    csv::Error,
    kermit_iters::Joinable,
    std::{fs::File, path::Path, str::FromStr},
};

pub enum ModelType {
    Positional,
    Named,
}

#[derive(Clone, Debug)]
pub struct RelationHeader {
    name: String,
    attrs: Vec<String>,
    arity: usize,
}

impl RelationHeader {
    /// Creates a new `RelationHeader` with the specified name, attributes, and
    /// arity.
    pub fn new(name: impl Into<String>, attrs: Vec<String>) -> Self {
        let arity = attrs.len();
        RelationHeader {
            name: name.into(),
            attrs,
            arity,
        }
    }

    pub fn new_nameless(attrs: Vec<String>) -> Self {
        let arity = attrs.len();
        RelationHeader {
            name: String::new(),
            attrs,
            arity,
        }
    }

    pub fn new_positional(name: impl Into<String>, arity: usize) -> Self {
        RelationHeader {
            name: name.into(),
            attrs: vec![],
            arity,
        }
    }

    pub fn new_nameless_positional(arity: usize) -> Self {
        RelationHeader {
            name: String::new(),
            attrs: vec![],
            arity,
        }
    }

    pub fn is_nameless(&self) -> bool { self.name.is_empty() }

    pub fn name(&self) -> &str { &self.name }

    pub fn attrs(&self) -> &[String] { &self.attrs }

    pub fn arity(&self) -> usize { self.arity }

    pub fn model_type(&self) -> ModelType {
        if self.attrs.is_empty() {
            ModelType::Positional
        } else {
            ModelType::Named
        }
    }
}

impl From<usize> for RelationHeader {
    fn from(value: usize) -> RelationHeader { RelationHeader::new_nameless_positional(value) }
}

pub trait Projectable {
    fn project(&self, columns: Vec<usize>) -> Self;
}

/// The `Relation` trait defines a relational data structure.
pub trait Relation: Joinable + Projectable {
    fn header(&self) -> &RelationHeader;

    /// Creates a new relation with the specified arity.
    fn new(header: RelationHeader) -> Self;

    /// Creates a new relation with the specified arity and given tuples.
    fn from_tuples(header: RelationHeader, tuples: Vec<Vec<Self::KT>>) -> Self;

    /// Inserts a tuple into the relation, returning `true` if successful and
    /// `false` if otherwise.
    fn insert(&mut self, tuple: Vec<Self::KT>) -> bool;

    /// Inserts multiple tuples into the relation, returning `true` if
    /// successful and `false` if otherwise.
    fn insert_all(&mut self, tuples: Vec<Vec<Self::KT>>) -> bool;

    /// Creates a new relation builder with the specified arity.
    fn builder(header: RelationHeader) -> impl RelationBuilder<Output = Self>
    where
        Self: Relation + Sized,
    {
        Builder::<Self>::new(header)
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

    /// Creates a new relation builder with the specified arity.
    fn new(header: RelationHeader) -> Self;

    /// Consumes the builder and returns the resulting relation.
    fn build(self) -> Self::Output;

    /// Adds a tuple to the relation being built.
    fn add_tuple(self, tuple: Vec<<Self::Output as Joinable>::KT>) -> Self;

    /// Adds multiple tuples to the relation being built.
    fn add_tuples(self, tuples: Vec<Vec<<Self::Output as Joinable>::KT>>) -> Self;
}

/// A concrete, default implementation of the `RelationBuilder` trait for a
/// specific relation type `R`.
pub struct Builder<R: Relation> {
    header: RelationHeader,
    tuples: Vec<Vec<R::KT>>,
}

/// Implementation of the `RelationBuilder` trait for the `Builder<R>` type.
impl<R: Relation> RelationBuilder for Builder<R> {
    type Output = R;

    fn new(header: RelationHeader) -> Self {
        Builder {
            header,
            tuples: vec![],
        }
    }

    fn build(self) -> Self::Output {
        let mut r = R::new(self.header);
        r.insert_all(self.tuples);
        r
    }

    fn add_tuple(mut self, tuple: Vec<<Self::Output as Joinable>::KT>) -> Self {
        self.tuples.push(tuple);
        self
    }

    fn add_tuples(mut self, tuples: Vec<Vec<<Self::Output as Joinable>::KT>>) -> Self {
        self.tuples.extend(tuples);
        self
    }
}

/// Extension trait for `RelationBuilder` to add CSV file reading capabilities.
#[allow(dead_code)]
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
    <T::Output as Joinable>::KT: FromStr,
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
            let mut tuple: Vec<<T::Output as Joinable>::KT> = vec![];
            for x in record.iter() {
                if let Ok(y) = x.to_string().parse::<<T::Output as Joinable>::KT>() {
                    tuple.push(y);
                }
            }
            self = self.add_tuple(tuple);
        }
        Ok(self)
    }
}
