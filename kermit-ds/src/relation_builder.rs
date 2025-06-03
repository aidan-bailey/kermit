use {
    crate::relation::Relation,
    csv::Error,
    std::{fs::File, path::Path},
};

pub trait RelationBuilder {
    type Output: Relation;
    fn new(cardinality: usize) -> Self;
    fn build(self) -> Self::Output;
    fn add_tuple(self, tuple: Vec<<Self::Output as Relation>::KT>) -> Self;
    fn add_tuples(self, tuple: Vec<Vec<<Self::Output as Relation>::KT>>) -> Self;
}

pub trait RelationBuilderFileExt: RelationBuilder {
    fn add_csv<P: AsRef<Path>>(self, filepath: P, delimiter: u8) -> Result<Self, Error>
    where
        Self: Sized;
}

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
