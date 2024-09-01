use {
    crate::{
        ds::{relation::Relation, relation_builder::RelationBuilder, relation_trie::{trie::RelationTrie, trie_builder::TrieBuilder}},
        iters::trie::TrieIterable,
        kvs::keyvalstore::KeyValStore,
    },
    std::{collections::HashMap, fmt::Debug, hash::Hash, str::FromStr},
};

pub trait RelationTrait<'a, KT: Debug + FromStr + std::cmp::PartialOrd + std::clone::Clone>:
    Relation<KT> + TrieIterable<'a, KT>
{
}

impl<'a, KT, T> RelationTrait<'a, KT> for T
where
    KT: Debug + FromStr +  std::cmp::PartialOrd + std::clone::Clone,
    T: Relation<KT> + TrieIterable<'a, KT>,
{
}

pub struct Database<'a, KT, VT, KVST>
where
    KT: Debug + FromStr +  PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
    KVST: KeyValStore<KT, VT>,
    VT: Hash,
{
    name: String,
    relations: HashMap<String, Box<dyn RelationTrait<'a, KT> + 'a>>,
    store: KVST,
    phantom_vt: std::marker::PhantomData<VT>,
}

impl<'a, KT, VT, KVST> Database<'a, KT, VT, KVST>
where
    KT: Debug + FromStr +  PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
    KVST: KeyValStore<KT, VT>,
    VT: Hash,
{
    pub fn new(name: String, store: KVST) -> Self {
        Database {
            name,
            relations: HashMap::new(),
            store,
            phantom_vt: std::marker::PhantomData,
        }
    }

    pub fn name(&self) -> &String { &self.name }

    pub fn add_relation<B, R>(&mut self, name: String, cardinality: usize) where
        B: RelationBuilder<KT, R>,
        R: RelationTrait<'a, KT> + 'a,{
        let relation = Box::new(B::new(cardinality).build());
        self.relations.insert(name, relation);
    }

    pub fn add_tuple(&mut self, relation_name: &String, tuple: Vec<VT>) {
        let keys = self.store.add_all(tuple);
        self.relations.get_mut(relation_name).unwrap().insert(keys);
    }
}

#[cfg(test)]
mod tests {

    use {
        super::*,
        crate::{
            ds::relation_trie::trie_builder::TrieBuilder,
            kvs::{anyvaltype::AnyValType, naivestore::NaiveStore},
        },
    };

    #[test]
    fn test_relation() {
        let mut db = Database::new("test".to_string(), NaiveStore::<AnyValType, _>::default());
        let relation_name = "apple".to_string();
        db.add_relation::<TrieBuilder<_>, _>(relation_name.clone(), 3);
        let tuple = vec![AnyValType::from("Apple"), AnyValType::from(1), AnyValType::from(2)];
        db.add_tuple(&relation_name, tuple)
    }
}
