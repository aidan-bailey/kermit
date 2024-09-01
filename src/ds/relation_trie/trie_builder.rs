use {
    crate::ds::relation_trie::trie::RelationTrie,
    csv::Error,
    std::{fmt::Debug, fs::File, path::Path, str::FromStr},
};

pub struct TrieBuilder<KT: PartialOrd + PartialEq + Clone + FromStr + Debug> {
    cardinality: usize,
    tuples: Vec<Vec<KT>>,
}

impl<KT: PartialOrd + PartialEq + Clone + FromStr + Debug> TrieBuilder<KT> {
    pub fn new(cardinality: usize) -> TrieBuilder<KT> {
        TrieBuilder {
            cardinality,
            tuples: vec![],
        }
    }

    pub fn build(self) -> RelationTrie<KT> {
        RelationTrie::from_mut_tuples(self.cardinality, self.tuples)
    }

    pub fn add_tuple(mut self, tuple: Vec<KT>) -> TrieBuilder<KT> {
        self.tuples.push(tuple);
        self
    }

    pub fn add_tuples(mut self, tuples: Vec<Vec<KT>>) -> TrieBuilder<KT> {
        self.tuples.extend(tuples);
        self
    }

    pub fn from_file<P: AsRef<Path>>(mut self, filepath: P) -> Result<TrieBuilder<KT>, Error> {
        let file = File::open(filepath)?;
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(b',')
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
            self.tuples.push(tuple);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {

    // use crate::ds::relation_trie::{node::TrieFields,
    // trie_builder::TrieBuilder};
    //
    // Read from file
    // #[test]
    // fn trie_builder_read_from_file() {
    // let trie = TrieBuilder::<String>::new(3)
    // .from_file::<&str>("test.csv")
    // .unwrap()
    // .build();
    // assert_eq!(trie.children()[0].key(), "1");
    // assert_eq!(trie.children()[1].key(), "3");
    // assert_eq!(trie.children()[0].children()[0].key(), "3");
    // assert_eq!(trie.children()[0].children()[1].key(), "4");
    // assert_eq!(trie.children()[0].children()[2].key(), "5");
    // assert_eq!(trie.children()[1].children()[0].key(), "5");
    // assert_eq!(trie.children()[0].children()[0].children()[0].key(), "4");
    // assert_eq!(trie.children()[0].children()[0].children()[1].key(), "5");
    // assert_eq!(trie.children()[0].children()[1].children()[0].key(), "6");
    // assert_eq!(trie.children()[0].children()[1].children()[1].key(), "8");
    // assert_eq!(trie.children()[0].children()[1].children()[2].key(), "9");
    // assert_eq!(trie.children()[0].children()[2].children()[0].key(), "2");
    // assert_eq!(trie.children()[1].children()[0].children()[0].key(), "2");
    // }
}
