use {
    crate::relation::Relation,
    csv::Error,
    std::{fmt::Debug, fs::File, path::Path, str::FromStr},
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

pub trait RelationBuilderFileExt<KT, R>: RelationBuilder<KT, R>
where
    KT: PartialOrd + PartialEq + Clone + FromStr + Debug,
    R: Relation<KT>,
{
    fn add_csv<P: AsRef<Path>>(self, filepath: P, delimiter: u8) -> Result<Self, Error>
    where
        Self: Sized;
}

impl<KT, R, T> RelationBuilderFileExt<KT, R> for T
where
    KT: PartialOrd + PartialEq + Clone + FromStr + Debug,
    R: Relation<KT>,
    T: RelationBuilder<KT, R>
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
            let mut tuple: Vec<KT> = vec![];
            for x in record.iter() {
                if let Ok(y) = x.to_string().parse::<KT>() {
                    tuple.push(y);
                }
            }
            self = self.add_tuple(tuple);
        }
        Ok(self)
    }
}
