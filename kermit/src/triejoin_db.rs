/*
use {
    crate::{
        algos::join_algo::JoinAlgo,
        ds::{
            relation::Relation,
            relation_builder::RelationBuilder,
            relation_trie::{trie::RelationTrie, trie_builder::TrieBuilder},
        },
        kvs::keyvalstore::KeyValStore,
    },
    kermit_algos::leapfrog_triejoin::LeapfrogTriejoinIter,
    kermit_ds::relation_trie::trie_iter::TrieIter,
    kermit_iters::JoinIterator,
    std::{collections::HashMap, fmt::Debug, hash::Hash, str::FromStr},
};

pub struct LFTJDatabase<KT, VT, KVST>
where
    KT: Debug + FromStr + PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
    KVST: KeyValStore<KT, VT>,
    VT: Hash,
{
    name: String,
    relations: HashMap<String, RelationTrie<KT>>,
    store: KVST,
    phantom_vt: std::marker::PhantomData<VT>,
    phantom_kt: std::marker::PhantomData<KT>,
}

impl<KT, VT, KVST> LFTJDatabase<KT, VT, KVST>
where
    KT: Debug + FromStr + PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
    KVST: KeyValStore<KT, VT>,
    VT: Hash,
{
    pub fn new(name: String, store: KVST) -> Self {
        LFTJDatabase {
            name,
            relations: HashMap::new(),
            store,
            phantom_vt: std::marker::PhantomData,
            phantom_kt: std::marker::PhantomData,
        }
    }

    pub fn name(&self) -> &String { &self.name }

    pub fn add_relation(&mut self, name: String, cardinality: usize) {
        let relation = TrieBuilder::new(cardinality).build();
        self.relations.insert(name, relation);
    }

    pub fn add_tuple(&mut self, relation_name: &String, tuple: Vec<VT>) {
        let keys = self.store.add_all(tuple);
        self.relations.get_mut(relation_name).unwrap().insert(keys);
    }

    pub fn join(
        &self, relations: Vec<String>, variables: Vec<usize>, rel_variables: Vec<Vec<usize>>, output_relation: String,
    ) {
        let iters = relations
            .iter()
            .map(|rel_name| {
                let rel = self.relations.get(rel_name).unwrap();
                let mut iter = TrieIter::new(rel);
                iter
            })
            .collect::<Vec<TrieIter<KT>>>();
        let lftj_iter = LeapfrogTriejoinIter::new(variables, rel_variables, iters);
    }
}

#[cfg(test)]
mod tests {

    use {
        super::*,
        crate::{
            ds::relation_trie::{trie::RelationTrie, trie_builder::TrieBuilder},
            kvs::{anyvaltype::AnyValType, naivestore::NaiveStore},
        },
    };

    #[test]
    fn test_relation() {
        let mut db: LFTJDatabase<
            u64,
            AnyValType,
            NaiveStore<_, std::hash::BuildHasherDefault<std::hash::DefaultHasher>>,
        > = LFTJDatabase::new("test".to_string(), NaiveStore::<AnyValType, _>::default());
        let relation_name = "apple".to_string();
        db.add_relation(relation_name.clone(), 3);
        let tuple = vec![
            AnyValType::from("Apple"),
            AnyValType::from(1),
            AnyValType::from(2),
        ];
        db.add_tuple(&relation_name, tuple)
    }
}
*/
