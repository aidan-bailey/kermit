use {
    super::relation_trie::RelationTrie,
    crate::{key_type::KeyType, relation_builder::RelationBuilder},
};

pub struct RelationTrieBuilder<KT: KeyType> {
    cardinality: usize,
    tuples: Vec<Vec<KT>>,
}

impl<KT: KeyType> RelationBuilder for RelationTrieBuilder<KT> {
    type Output = RelationTrie<KT>;

    fn new(cardinality: usize) -> Self {
        RelationTrieBuilder {
            cardinality,
            tuples: vec![],
        }
    }

    fn build(self) -> Self::Output { RelationTrie::from_mut_tuples(self.cardinality, self.tuples) }

    fn add_tuple(mut self, tuple: Vec<KT>) -> Self {
        self.tuples.push(tuple);
        self
    }

    fn add_tuples(mut self, tuples: Vec<Vec<KT>>) -> Self {
        self.tuples.extend(tuples);
        self
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
