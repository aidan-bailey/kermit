use {
    crate::ds::relation::Relation,
    csv::Error,
    std::{fmt::Debug, path::Path, str::FromStr},
};

pub trait RelationBuilder<KT, R>
where
    KT: PartialOrd + PartialEq + Clone + FromStr + Debug,
    R: Relation<KT>,
{
    fn new(cardinality: usize) -> Self;
    fn build(self) -> R;
    fn add_tuple(self, tuple: Vec<KT>) -> Self;
    fn add_tuples(self, tuple: Vec<Vec<KT>>) -> Self;
    fn add_file<P: AsRef<Path>>(self, filepath: P) -> Result<Self, Error>
    where
        Self: Sized;
}
