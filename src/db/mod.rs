use {
    crate::{ds::relation::Relation, iters::trie::TrieIterable, kvs::keyvalstore::KeyValStore},
    std::{collections::HashMap, hash::Hash},
};

pub trait RelationTrait<'a, KT: std::cmp::PartialOrd + std::clone::Clone>:
    Relation<KT> + TrieIterable<'a, KT>
{
}

impl<'a, KT, T> RelationTrait<'a, KT> for T
where
    KT: std::cmp::PartialOrd + std::clone::Clone,
    T: Relation<KT> + TrieIterable<'a, KT>,
{
}

pub struct Database<'a, KT, VT, KVST>
where
    KT: PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
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
    KT: PartialOrd + PartialEq + Clone + Hash + std::cmp::Eq,
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

    pub fn add_relation(&mut self, name: String, relation: impl RelationTrait<'a, KT> + 'a) {
        self.relations.insert(name, Box::new(relation));
    }

    pub fn add_tuple(&mut self, relation_name: String, tuple: Vec<VT>) {
        let keys = self.store.add_all(tuple);
        self.relations.get_mut(&relation_name).unwrap().insert(keys);
    }
}

#[cfg(test)]
mod tests {

    use {
        super::*,
        crate::{
            ds::{relation_builder::RelationBuilder, relation_trie::trie_builder::TrieBuilder},
            kvs::{anyvaltype::AnyValType, naivestore::NaiveStore},
        },
    };

    #[test]
    fn test_classic() {
        let mut db = Database::new("test".to_string(), NaiveStore::<AnyValType, _>::default());

        let t1 = TrieBuilder::new(1)
            .add_tuples(vec![vec![1], vec![2], vec![3]])
            .build();
        db.add_relation("apple".to_string(), t1);
    }
}
